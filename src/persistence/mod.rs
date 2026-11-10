//! Injected persistence contracts and use cases; only `d1` depends on Workers.

mod audit;
pub mod d1;
mod error;
mod export;
pub(crate) mod report_scan;
mod report_total;
mod reporting;
mod repositories;
mod types;
mod use_cases;

pub use audit::{
    AuditChange, AuditEntry, AuditKind, AuditReason, AuditRepository, AuditRevision, AuditTarget,
    MAX_AUDIT_REASON_CHARS,
};
pub use error::{PersistenceError, PersistenceResult};
pub use export::{
    ExportCursor, ExportRepository, ExportRequest, EXPORT_FORMAT, MAX_EXPORT_AUDITS_PER_CHUNK,
    MAX_EXPORT_CATALOGS_PER_CHUNK,
};
pub use report_total::ReportTotal;
pub use reporting::{
    AuditPageRequest, AuditedSessionState, GameLeaderboard, LeaderboardEntry, PlayerReport,
    PlayerSessionSummary, ReportPage, ReportPageRequest, ReportParticipantIdentity,
    ReportingRepository, SessionAudit, SessionAuditPage, SessionDetail, SessionDetailEntry,
    SessionSummary, MAX_AUDIT_PAGE_SIZE, MAX_REPORT_PAGE_NUMBER, MAX_REPORT_PAGE_SIZE,
};
pub use repositories::{
    ConfirmationRepository, DraftRepository, GameRepository, RosterRepository, SessionRepository,
    UpdateRepository,
};
pub use types::{Confirmation, DraftState, OwnerId, PersistentDraft, Revision, UpdateKey};
pub use use_cases::{ConfirmCommand, Ledger};

#[cfg(test)]
mod tests;
