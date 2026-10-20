//! Pure ledger rules. Dates and creation instants are supplied by the caller.

mod dates;
mod entities;
mod error;
mod identity;
mod money;
mod participant;
mod session;

pub use dates::{CreatedAt, OccurredOn};
pub use entities::{DisplayName, Game, Guest, Player, RosterStatus};
pub use error::DomainError;
pub use identity::{GameId, GuestId, PlayerId, SessionId};
pub use money::Cents;
pub use participant::{AmountEntry, Participant, ParticipantId};
pub use session::{ConfirmedSession, DraftSession, SessionState};

#[cfg(test)]
mod tests;
