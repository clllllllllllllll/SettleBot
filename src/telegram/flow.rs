use super::{
    commands::{amount, Action},
    Error, Result,
};
use crate::domain::{
    AmountEntry, Cents, DisplayName, DraftSession, Game, GameId, Guest, GuestId, OccurredOn,
    Participant, Player, PlayerId, RosterStatus, SessionId,
};
use crate::persistence::{DraftState, OwnerId, PersistentDraft, Revision};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
pub(crate) enum Stage {
    Game,
    Date,
    Participants,
    Amounts,
    Review,
    Confirmed,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Choice {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Selection {
    pub choice: Choice,
    pub guest: bool,
    pub cents: Option<i64>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Flow {
    pub id: String,
    pub owner: i64,
    pub chat: i64,
    pub revision: u32,
    pub stage: Stage,
    pub games: Vec<Choice>,
    pub roster: Vec<Choice>,
    pub game: Option<Choice>,
    pub date: Option<String>,
    pub selected: Vec<Selection>,
}

impl Flow {
    pub fn storage_state(&self) -> &'static str {
        if self.stage == Stage::Confirmed {
            "consumed"
        } else {
            "open"
        }
    }

    pub fn validate(&self) -> Result<()> {
        SessionId::new(&self.id)?;
        OwnerId::new(self.owner)?;
        if self.chat >= 0 {
            return Err(Error::Invalid);
        }
        for choice in self.games.iter().chain(&self.roster) {
            choice.validate()?;
        }
        if let Some(game) = &self.game {
            game.validate()?;
        }
        if let Some(date) = &self.date {
            OccurredOn::parse(date)?;
        }
        self.validate_stage()
    }

    fn validate_stage(&self) -> Result<()> {
        match self.stage {
            Stage::Game
                if self.game.is_none() && self.date.is_none() && self.selected.is_empty() =>
            {
                Ok(())
            }
            Stage::Date
                if self.game.is_some() && self.date.is_none() && self.selected.is_empty() =>
            {
                Ok(())
            }
            Stage::Participants | Stage::Amounts => self.session().map(|_| ()),
            Stage::Review | Stage::Confirmed => self.draft().map(|_| ()),
            _ => Err(Error::Invalid),
        }
    }

    pub fn new(
        id: String,
        owner: OwnerId,
        chat: i64,
        games: Vec<Game>,
        players: Vec<Player>,
    ) -> Self {
        Self {
            id,
            owner: owner.value(),
            chat,
            revision: 0,
            stage: Stage::Game,
            games: games.iter().map(Choice::game).collect(),
            roster: roster_choices(&players),
            game: None,
            date: None,
            selected: Vec::new(),
        }
    }

    pub fn check(&self, owner: OwnerId, chat: i64, revision: Revision) -> Result<()> {
        if self.owner != owner.value() || self.chat != chat {
            return Err(Error::Rejected);
        }
        if self.revision != revision.value() || self.stage == Stage::Confirmed {
            return Err(Error::Stale);
        }
        Ok(())
    }

    pub(super) fn advance(&mut self, action: Action) -> Result<()> {
        match (self.stage, action) {
            (Stage::Game, Action::Game(index)) => self.choose_game(index)?,
            (Stage::Date, Action::Text(value)) => self.choose_date(&value)?,
            (Stage::Participants, Action::Roster(index)) => self.toggle_player(index)?,
            (Stage::Participants, Action::Guest(name)) => self.add_guest(&name)?,
            (Stage::Participants, Action::Done) => self.start_amounts()?,
            (Stage::Amounts, Action::Text(value)) => self.enter_amount(&value)?,
            (Stage::Review, Action::Edit) => self.start_amounts()?,
            (Stage::Review, Action::Confirm) => self.stage = Stage::Confirmed,
            _ => return Err(Error::Stale),
        }
        self.revision = Revision::new(self.revision).next()?.value();
        Ok(())
    }

    fn choose_game(&mut self, index: usize) -> Result<()> {
        self.game = Some(self.games.get(index).ok_or(Error::Invalid)?.clone());
        self.stage = Stage::Date;
        Ok(())
    }

    fn choose_date(&mut self, value: &str) -> Result<()> {
        self.date = Some(OccurredOn::parse(value)?.to_string());
        self.stage = Stage::Participants;
        Ok(())
    }

    fn toggle_player(&mut self, index: usize) -> Result<()> {
        let choice = self.roster.get(index).ok_or(Error::Invalid)?.clone();
        if let Some(position) = self
            .selected
            .iter()
            .position(|p| !p.guest && p.choice.id == choice.id)
        {
            self.selected.remove(position);
            return Ok(());
        }
        self.selected.push(Selection {
            choice,
            guest: false,
            cents: None,
        });
        Ok(())
    }

    fn add_guest(&mut self, name: &str) -> Result<()> {
        let name = DisplayName::new(name)?.as_str().to_owned();
        let id = format!("guest_{}", self.revision);
        self.selected.push(Selection {
            choice: Choice { id, name },
            guest: true,
            cents: None,
        });
        Ok(())
    }

    fn start_amounts(&mut self) -> Result<()> {
        if self.selected.len() < 2 {
            return Err(Error::Invalid);
        }
        self.selected.iter_mut().for_each(|p| p.cents = None);
        self.stage = Stage::Amounts;
        Ok(())
    }

    fn enter_amount(&mut self, value: &str) -> Result<()> {
        self.selected
            .iter_mut()
            .find(|p| p.cents.is_none())
            .ok_or(Error::Stale)?
            .cents = Some(amount(value)?.value());
        if self.selected.iter().all(|p| p.cents.is_some()) {
            self.stage = Stage::Review;
        }
        Ok(())
    }

    pub fn draft(&self) -> Result<PersistentDraft> {
        let session = self.session()?;
        Ok(PersistentDraft::new(
            session,
            OwnerId::new(self.owner)?,
            self.entries()?,
            Revision::default(),
            DraftState::Open,
        )?)
    }

    fn session(&self) -> Result<DraftSession> {
        let id = SessionId::new(&self.id)?;
        let participants = self
            .selected
            .iter()
            .map(|p| p.participant(&id))
            .collect::<Result<Vec<_>>>()?;
        let game = GameId::new(&self.game.as_ref().ok_or(Error::Invalid)?.id)?;
        let date = OccurredOn::parse(self.date.as_deref().ok_or(Error::Invalid)?)?;
        Ok(DraftSession::new(id, game, date, participants)?)
    }

    fn entries(&self) -> Result<Vec<AmountEntry>> {
        let id = SessionId::new(&self.id)?;
        self.selected
            .iter()
            .map(|selection| {
                let participant = selection.participant(&id)?;
                let amount = Cents::new(selection.cents.ok_or(Error::Invalid)?);
                Ok(AmountEntry::new(participant.id(), amount))
            })
            .collect()
    }

    pub fn total(&self) -> Result<Cents> {
        let amounts = self
            .selected
            .iter()
            .map(|p| p.cents.map(Cents::new).ok_or(Error::Invalid))
            .collect::<Result<Vec<_>>>()?;
        Ok(Cents::checked_sum(amounts)?)
    }
}

fn roster_choices(players: &[Player]) -> Vec<Choice> {
    players
        .iter()
        .filter(|player| player.status() == RosterStatus::Active)
        .map(|player| Choice {
            id: player.id().as_str().into(),
            name: player.name().as_str().into(),
        })
        .collect()
}

impl Choice {
    fn validate(&self) -> Result<()> {
        PlayerId::new(&self.id)?;
        DisplayName::new(&self.name)?;
        Ok(())
    }
    fn game(game: &Game) -> Self {
        Self {
            id: game.id().as_str().into(),
            name: game.name().as_str().into(),
        }
    }
}

impl Selection {
    fn participant(&self, id: &SessionId) -> Result<Participant> {
        if !self.guest {
            return Ok(Participant::Roster(PlayerId::new(&self.choice.id)?));
        }
        Ok(Participant::Guest(Guest::new(
            GuestId::new(id.clone(), &self.choice.id)?,
            DisplayName::new(&self.choice.name)?,
        )))
    }

    pub fn label(&self) -> String {
        if self.guest {
            return format!("{} (one-time guest)", self.choice.name);
        }
        format!("{} [{}]", self.choice.name, self.choice.id)
    }
}
