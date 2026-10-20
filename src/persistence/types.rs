use std::collections::BTreeSet;

use crate::domain::{
    AmountEntry, ConfirmedSession, CreatedAt, DomainError, DraftSession, Participant, SessionId,
};

use super::{PersistenceError, PersistenceResult};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnerId(i64);

impl OwnerId {
    pub fn new(value: i64) -> PersistenceResult<Self> {
        if value <= 0 {
            return Err(PersistenceError::InvalidOwner);
        }
        Ok(Self(value))
    }

    pub fn value(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateKey(String);

impl UpdateKey {
    pub fn new(value: &str) -> PersistenceResult<Self> {
        if value.is_empty() || !value.bytes().all(is_key_byte) {
            return Err(PersistenceError::InvalidUpdateKey);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_key_byte(value: u8) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_')
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Revision(u32);

impl Revision {
    pub fn new(value: u32) -> Self {
        Self(value)
    }

    pub fn value(self) -> u32 {
        self.0
    }

    pub fn next(self) -> PersistenceResult<Self> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(PersistenceError::RevisionOverflow)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DraftState {
    Open,
    Consumed,
}

#[derive(Debug, Eq, PartialEq)]
pub struct PersistentDraft {
    session: DraftSession,
    owner: OwnerId,
    entries: Vec<AmountEntry>,
    revision: Revision,
    state: DraftState,
}

impl PersistentDraft {
    pub fn new(
        session: DraftSession,
        owner: OwnerId,
        entries: Vec<AmountEntry>,
        revision: Revision,
        state: DraftState,
    ) -> PersistenceResult<Self> {
        validate_partial_entries(&session, &entries)?;
        Ok(Self {
            session,
            owner,
            entries,
            revision,
            state,
        })
    }

    pub fn session(&self) -> &DraftSession {
        &self.session
    }

    pub fn owner(&self) -> OwnerId {
        self.owner
    }

    pub fn entries(&self) -> &[AmountEntry] {
        &self.entries
    }

    pub fn revision(&self) -> Revision {
        self.revision
    }

    pub fn state(&self) -> DraftState {
        self.state
    }

    pub fn check_edit(&self, owner: OwnerId, revision: Revision) -> PersistenceResult<()> {
        if self.owner != owner {
            return Err(PersistenceError::OwnerMismatch);
        }
        if self.state != DraftState::Open {
            return Err(PersistenceError::DraftConsumed);
        }
        if self.revision != revision {
            return Err(PersistenceError::RevisionConflict);
        }
        Ok(())
    }

    pub fn confirm(self, created_at: CreatedAt) -> PersistenceResult<ConfirmedSession> {
        Ok(self.session.confirm(self.entries, created_at)?)
    }
}

fn validate_partial_entries(
    session: &DraftSession,
    entries: &[AmountEntry],
) -> PersistenceResult<()> {
    let participants = session.participants();
    let selected: BTreeSet<_> = participants.iter().map(Participant::id).collect();
    let mut entered = BTreeSet::new();
    for entry in entries {
        if !selected.contains(entry.participant_id()) {
            return Err(DomainError::UnselectedParticipant(entry.participant_id().clone()).into());
        }
        if !entered.insert(entry.participant_id()) {
            return Err(DomainError::DuplicateAmount(entry.participant_id().clone()).into());
        }
    }
    Ok(())
}

/// The durable response to one successful update; replay returns this original instant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Confirmation {
    pub session_id: SessionId,
    pub owner: OwnerId,
    pub created_at: CreatedAt,
}
