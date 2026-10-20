//! Injected persistence contracts and use cases; only `d1` depends on Workers.

pub mod d1;
mod error;
mod repositories;
mod types;
mod use_cases;

pub use error::{PersistenceError, PersistenceResult};
pub use repositories::{
    ConfirmationRepository, DraftRepository, GameRepository, RosterRepository, SessionRepository,
    UpdateRepository,
};
pub use types::{Confirmation, DraftState, OwnerId, PersistentDraft, Revision, UpdateKey};
pub use use_cases::{ConfirmCommand, Ledger};

#[cfg(test)]
mod tests;
