use std::fmt;

use super::{Cents, ParticipantId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DomainError {
    RequiredValue(&'static str),
    InvalidIdentifier(&'static str),
    InvalidDisplayName,
    InvalidOccurredOn,
    InvalidCreatedAt,
    GuestOutsideSession,
    DuplicateParticipant(ParticipantId),
    TooFewParticipants,
    UnselectedParticipant(ParticipantId),
    DuplicateAmount(ParticipantId),
    MissingAmount(ParticipantId),
    AmountOverflow,
    NonZeroTotal(Cents),
}

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequiredValue(field) => write!(formatter, "{field} is required"),
            Self::InvalidIdentifier(field) => write!(formatter, "{field} is invalid"),
            Self::InvalidDisplayName => formatter.write_str("display name is invalid"),
            Self::InvalidOccurredOn => formatter.write_str("played date is invalid"),
            Self::InvalidCreatedAt => formatter.write_str("UTC creation instant is invalid"),
            Self::GuestOutsideSession => formatter.write_str("guest belongs to another session"),
            Self::DuplicateParticipant(_) => formatter.write_str("participant is selected twice"),
            Self::TooFewParticipants => formatter.write_str("two or more participants required"),
            Self::UnselectedParticipant(_) => {
                formatter.write_str("unselected identity has an amount")
            }
            Self::DuplicateAmount(_) => formatter.write_str("participant has more than one amount"),
            Self::MissingAmount(_) => formatter.write_str("selected participant has no amount"),
            Self::AmountOverflow => formatter.write_str("total exceeds signed integer cents"),
            Self::NonZeroTotal(_) => formatter.write_str("session total must be zero cents"),
        }
    }
}

impl std::error::Error for DomainError {}
