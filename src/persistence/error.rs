use std::fmt;

use crate::domain::DomainError;

pub type PersistenceResult<T> = Result<T, PersistenceError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersistenceError {
    Invalid(DomainError),
    InvalidOwner,
    InvalidUpdateKey,
    RevisionOverflow,
    Corrupt(&'static str),
    Storage,
    NotFound,
    IdentityConflict,
    OwnerMismatch,
    RevisionConflict,
    DraftConsumed,
    UpdateKeyConflict,
}

impl From<DomainError> for PersistenceError {
    fn from(error: DomainError) -> Self {
        Self::Invalid(error)
    }
}

impl fmt::Display for PersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PersistenceError {}
