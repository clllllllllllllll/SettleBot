use serde::Deserialize;
use worker::{
    D1Type,
    D1Type::{Null, Text},
};

use crate::domain::{
    AmountEntry, Cents, ConfirmedSession, CreatedAt, DisplayName, DraftSession, Game, GameId,
    Guest, GuestId, OccurredOn, Participant, Player, PlayerId, RosterStatus, SessionId,
};
use crate::persistence::{
    Confirmation, DraftState, OwnerId, PersistenceError, PersistenceResult, PersistentDraft,
    Revision,
};

#[derive(Deserialize)]
pub(super) struct PlayerRow {
    id: String,
    display_name: String,
    status: String,
}

impl PlayerRow {
    pub fn decode(self) -> PersistenceResult<Player> {
        let status = match self.status.as_str() {
            "active" => RosterStatus::Active,
            "inactive" => RosterStatus::Inactive,
            _ => return Err(PersistenceError::Corrupt("roster status")),
        };
        Ok(Player::new(
            PlayerId::new(&self.id)?,
            DisplayName::new(&self.display_name)?,
            status,
        ))
    }
}

#[derive(Deserialize)]
pub(super) struct GameRow {
    id: String,
    display_name: String,
}

impl GameRow {
    pub fn decode(self) -> PersistenceResult<Game> {
        Ok(Game::new(
            GameId::new(&self.id)?,
            DisplayName::new(&self.display_name)?,
        ))
    }
}

#[derive(Deserialize)]
pub(super) struct DraftRow {
    pub id: String,
    pub owner_id: String,
    pub game_id: String,
    pub occurred_on: String,
    pub revision: u32,
    pub state: String,
}

impl DraftRow {
    pub fn decode(self, rows: Vec<ParticipantRow>) -> PersistenceResult<PersistentDraft> {
        let id = SessionId::new(&self.id)?;
        let (participants, entries) = decode_participants(&id, rows)?;
        let session = decode_selection(id, &self.game_id, &self.occurred_on, participants)?;
        PersistentDraft::new(
            session,
            decode_owner(&self.owner_id)?,
            entries,
            Revision::new(self.revision),
            decode_state(&self.state)?,
        )
    }
}

#[derive(Deserialize)]
pub(super) struct SessionRow {
    pub id: String,
    owner_id: String,
    game_id: String,
    occurred_on: String,
    created_at: String,
}

impl SessionRow {
    pub fn decode(self, rows: Vec<ParticipantRow>) -> PersistenceResult<ConfirmedSession> {
        let id = SessionId::new(&self.id)?;
        let (participants, entries) = decode_participants(&id, rows)?;
        let draft = decode_selection(id, &self.game_id, &self.occurred_on, participants)?;
        decode_owner(&self.owner_id)?;
        Ok(draft.confirm(entries, CreatedAt::parse(&self.created_at)?)?)
    }

    pub fn confirmation(self) -> PersistenceResult<Confirmation> {
        GameId::new(&self.game_id)?;
        OccurredOn::parse(&self.occurred_on)?;
        Ok(Confirmation {
            session_id: SessionId::new(&self.id)?,
            owner: decode_owner(&self.owner_id)?,
            created_at: CreatedAt::parse(&self.created_at)?,
        })
    }
}

fn decode_selection(
    id: SessionId,
    game: &str,
    date: &str,
    participants: Vec<Participant>,
) -> PersistenceResult<DraftSession> {
    Ok(DraftSession::new(
        id,
        GameId::new(game)?,
        OccurredOn::parse(date)?,
        participants,
    )?)
}

fn decode_state(value: &str) -> PersistenceResult<DraftState> {
    match value {
        "open" => Ok(DraftState::Open),
        "consumed" => Ok(DraftState::Consumed),
        _ => Err(PersistenceError::Corrupt("draft state")),
    }
}

fn decode_owner(value: &str) -> PersistenceResult<OwnerId> {
    let number = value
        .parse()
        .map_err(|_| PersistenceError::Corrupt("owner ID"))?;
    let owner = OwnerId::new(number)?;
    if owner.value().to_string() != value {
        return Err(PersistenceError::Corrupt("owner ID"));
    }
    Ok(owner)
}

#[derive(Deserialize)]
pub(super) struct ParticipantRow {
    kind: String,
    identity_id: String,
    roster_player_id: Option<String>,
    guest_name: Option<String>,
    position: usize,
    amount_cents: Option<String>,
}

impl ParticipantRow {
    fn participant(&self, session_id: &SessionId) -> PersistenceResult<Participant> {
        match self.kind.as_str() {
            "roster"
                if self.roster_player_id.as_ref() == Some(&self.identity_id)
                    && self.guest_name.is_none() =>
            {
                Ok(Participant::Roster(PlayerId::new(&self.identity_id)?))
            }
            "guest" if self.roster_player_id.is_none() => self.guest(session_id),
            _ => Err(PersistenceError::Corrupt("participant kind")),
        }
    }

    fn guest(&self, session_id: &SessionId) -> PersistenceResult<Participant> {
        let name = self
            .guest_name
            .as_deref()
            .ok_or(PersistenceError::Corrupt("guest name"))?;
        let guest = Guest::new(
            GuestId::new(session_id.clone(), &self.identity_id)?,
            DisplayName::new(name)?,
        );
        Ok(Participant::Guest(guest))
    }
}

fn decode_participants(
    id: &SessionId,
    rows: Vec<ParticipantRow>,
) -> PersistenceResult<(Vec<Participant>, Vec<AmountEntry>)> {
    let mut participants = Vec::new();
    let mut entries = Vec::new();
    for (position, row) in rows.into_iter().enumerate() {
        if position != row.position {
            return Err(PersistenceError::Corrupt("participant position"));
        }
        let participant = row.participant(id)?;
        if let Some(amount) = row.amount_cents {
            let amount = amount
                .parse()
                .map_err(|_| PersistenceError::Corrupt("integer cents"))?;
            entries.push(AmountEntry::new(participant.id(), Cents::new(amount)));
        }
        participants.push(participant);
    }
    Ok((participants, entries))
}

pub(super) fn participant_arguments<'a>(
    session: &'a str,
    participant: &'a Participant,
) -> Vec<D1Type<'a>> {
    let (kind, id, roster, name) = match participant {
        Participant::Roster(id) => ("roster", id.as_str(), Some(id.as_str()), None),
        Participant::Guest(guest) => (
            "guest",
            guest.id().as_str(),
            None,
            Some(guest.name().as_str()),
        ),
    };
    vec![
        Text(session),
        Text(kind),
        Text(id),
        roster.map_or(Null, Text),
        name.map_or(Null, Text),
    ]
}

pub(super) fn corrupt(error: PersistenceError) -> PersistenceError {
    match error {
        PersistenceError::Invalid(_) | PersistenceError::InvalidOwner => {
            PersistenceError::Corrupt("validated domain value")
        }
        error => error,
    }
}
