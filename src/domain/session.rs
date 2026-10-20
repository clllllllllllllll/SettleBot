use std::collections::BTreeSet;

use super::{
    AmountEntry, Cents, CreatedAt, DomainError, GameId, OccurredOn, Participant, ParticipantId,
    SessionId,
};

const MINIMUM_PARTICIPANTS: usize = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionState {
    Draft,
    Confirmed,
}

/// A selected game, played date and participants awaiting complete, balanced amounts.
#[derive(Debug, Eq, PartialEq)]
pub struct DraftSession {
    id: SessionId,
    game_id: GameId,
    occurred_on: OccurredOn,
    participants: Vec<Participant>,
}

impl DraftSession {
    pub fn new(
        id: SessionId,
        game_id: GameId,
        occurred_on: OccurredOn,
        participants: Vec<Participant>,
    ) -> Result<Self, DomainError> {
        validate_participants(&id, &participants)?;
        Ok(Self {
            id,
            game_id,
            occurred_on,
            participants,
        })
    }

    pub fn state(&self) -> SessionState {
        SessionState::Draft
    }

    pub fn id(&self) -> &SessionId {
        &self.id
    }

    pub fn game_id(&self) -> &GameId {
        &self.game_id
    }

    pub fn occurred_on(&self) -> OccurredOn {
        self.occurred_on
    }

    pub fn participants(&self) -> &[Participant] {
        &self.participants
    }

    pub fn confirm(
        self,
        entries: Vec<AmountEntry>,
        created_at: CreatedAt,
    ) -> Result<ConfirmedSession, DomainError> {
        if self.participants.len() < MINIMUM_PARTICIPANTS {
            return Err(DomainError::TooFewParticipants);
        }
        validate_entries(&self.participants, &entries)?;
        validate_zero_total(&entries)?;
        Ok(ConfirmedSession {
            draft: self,
            entries,
            created_at,
        })
    }
}

/// Only successful confirmation can construct a session with immutable ledger entries.
#[derive(Debug, Eq, PartialEq)]
pub struct ConfirmedSession {
    draft: DraftSession,
    entries: Vec<AmountEntry>,
    created_at: CreatedAt,
}

impl ConfirmedSession {
    pub fn state(&self) -> SessionState {
        SessionState::Confirmed
    }

    pub fn id(&self) -> &SessionId {
        &self.draft.id
    }

    pub fn game_id(&self) -> &GameId {
        &self.draft.game_id
    }

    pub fn occurred_on(&self) -> OccurredOn {
        self.draft.occurred_on
    }

    pub fn created_at(&self) -> CreatedAt {
        self.created_at
    }

    pub fn participants(&self) -> &[Participant] {
        &self.draft.participants
    }

    pub fn entries(&self) -> &[AmountEntry] {
        &self.entries
    }
}

fn validate_participants(id: &SessionId, participants: &[Participant]) -> Result<(), DomainError> {
    let mut selected = BTreeSet::new();
    for participant in participants {
        validate_guest_scope(id, participant)?;
        let participant_id = participant.id();
        if !selected.insert(participant_id.clone()) {
            return Err(DomainError::DuplicateParticipant(participant_id));
        }
    }
    Ok(())
}

fn validate_guest_scope(id: &SessionId, participant: &Participant) -> Result<(), DomainError> {
    if let Participant::Guest(guest) = participant {
        if guest.id().session_id() != id {
            return Err(DomainError::GuestOutsideSession);
        }
    }
    Ok(())
}

fn validate_entries(
    participants: &[Participant],
    entries: &[AmountEntry],
) -> Result<(), DomainError> {
    let selected: BTreeSet<_> = participants.iter().map(Participant::id).collect();
    let mut entered = BTreeSet::new();
    for entry in entries {
        validate_entry(&selected, &mut entered, entry)?;
    }
    if let Some(missing) = selected.difference(&entered).next() {
        return Err(DomainError::MissingAmount(missing.clone()));
    }
    Ok(())
}

fn validate_entry(
    selected: &BTreeSet<ParticipantId>,
    entered: &mut BTreeSet<ParticipantId>,
    entry: &AmountEntry,
) -> Result<(), DomainError> {
    let participant_id = entry.participant_id();
    if !selected.contains(participant_id) {
        return Err(DomainError::UnselectedParticipant(participant_id.clone()));
    }
    if !entered.insert(participant_id.clone()) {
        return Err(DomainError::DuplicateAmount(participant_id.clone()));
    }
    Ok(())
}

fn validate_zero_total(entries: &[AmountEntry]) -> Result<(), DomainError> {
    let total = Cents::checked_sum(entries.iter().map(AmountEntry::amount))?;
    if total != Cents::ZERO {
        return Err(DomainError::NonZeroTotal(total));
    }
    Ok(())
}
