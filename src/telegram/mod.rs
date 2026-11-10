//! Authorized Telegram application boundary; platform I/O lives in `worker`.
mod auth;
pub(crate) mod bot_api;
mod commands;
pub(crate) mod flow;
mod logging;
pub(crate) mod policy;
mod rate;
pub(crate) mod render;
mod reporting;
mod resilience;
mod service;
mod store;
mod transport;
pub(crate) mod worker;

pub use auth::Configuration;
pub use policy::{PageSize, MAX_PAGE_SIZE};
pub(crate) use rate::{admit_with_retry, RateLimitRequest, RateLimiter};
pub(crate) use service::handle;
pub use service::Clock;
pub(crate) use store::{Change, Receipt, Repository, Transaction};
#[cfg(feature = "d1-test")]
pub(crate) use transport::deliver;
#[cfg(feature = "d1-test")]
pub(crate) use transport::receive as transport_receive;
pub(crate) use transport::Transport;

use crate::{domain::DomainError, persistence::PersistenceError};

#[derive(Debug, PartialEq)]
pub enum Error {
    Unauthorized,
    Rejected,
    Invalid,
    Stale,
    Limit,
    RateLimited,
    Timeout,
    TelegramNetwork,
    TelegramAmbiguous,
    TelegramRateLimited { retry_after: Option<u64> },
    TelegramServer,
    TelegramRejected,
    TelegramAccess,
    Storage,
    Configuration,
}
pub type Result<T> = std::result::Result<T, Error>;

impl From<DomainError> for Error {
    fn from(_: DomainError) -> Self {
        Self::Invalid
    }
}

impl From<PersistenceError> for Error {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::Storage | PersistenceError::Corrupt(_) => Self::Storage,
            PersistenceError::InvalidPage => Self::Limit,
            _ => Self::Invalid,
        }
    }
}

#[cfg(test)]
mod tests;
