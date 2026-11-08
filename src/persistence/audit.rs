use crate::domain::{Cents, CreatedAt, DisplayName, PlayerId, SessionId};

use super::{OwnerId, PersistenceError, PersistenceResult, ReportParticipantIdentity, UpdateKey};

pub const MAX_AUDIT_REASON_CHARS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditKind {
    Correction,
    Void,
}

impl AuditKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Correction => "correction",
            Self::Void => "void",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditReason(String);

impl AuditReason {
    pub fn new(value: &str) -> PersistenceResult<Self> {
        let value = value.trim();
        if value.is_empty()
            || value.chars().count() > MAX_AUDIT_REASON_CHARS
            || value.chars().any(char::is_control)
        {
            return Err(PersistenceError::InvalidAuditReason);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditEntry {
    pub participant: ReportParticipantIdentity,
    pub display_name: DisplayName,
    pub amount: Cents,
}

impl AuditEntry {
    pub(crate) fn identity_id(&self) -> &str {
        match &self.participant {
            ReportParticipantIdentity::Roster(id) => id.as_str(),
            ReportParticipantIdentity::Guest(id) => id.as_str(),
        }
    }

    pub(crate) fn roster_id(&self) -> Option<&PlayerId> {
        match &self.participant {
            ReportParticipantIdentity::Roster(id) => Some(id),
            ReportParticipantIdentity::Guest(_) => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditChange {
    pub key: UpdateKey,
    pub session_id: SessionId,
    pub actor: OwnerId,
    pub kind: AuditKind,
    pub reason: AuditReason,
    pub created_at: CreatedAt,
    pub entries: Vec<AuditEntry>,
}

impl AuditChange {
    pub fn correction(
        key: UpdateKey,
        session_id: SessionId,
        actor: OwnerId,
        reason: AuditReason,
        created_at: CreatedAt,
        entries: Vec<AuditEntry>,
    ) -> PersistenceResult<Self> {
        validate_entries(&entries)?;
        Ok(Self {
            key,
            session_id,
            actor,
            kind: AuditKind::Correction,
            reason,
            created_at,
            entries,
        })
    }

    pub fn void(
        key: UpdateKey,
        session_id: SessionId,
        actor: OwnerId,
        reason: AuditReason,
        created_at: CreatedAt,
    ) -> Self {
        Self {
            key,
            session_id,
            actor,
            kind: AuditKind::Void,
            reason,
            created_at,
            entries: Vec::new(),
        }
    }
}

fn validate_entries(entries: &[AuditEntry]) -> PersistenceResult<()> {
    if entries.len() < 2 {
        return Err(PersistenceError::InvalidAuditEntries);
    }
    let total = Cents::checked_sum(entries.iter().map(|entry| entry.amount))?;
    if total != Cents::ZERO {
        return Err(PersistenceError::InvalidAuditEntries);
    }
    Ok(())
}

#[allow(async_fn_in_trait)]
pub trait AuditRepository {
    async fn commit_audit(&self, change: &AuditChange) -> PersistenceResult<()>;
}
