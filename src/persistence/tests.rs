use crate::domain::{
    AmountEntry, Cents, DisplayName, DraftSession, GameId, Guest, GuestId, OccurredOn, Participant,
    PlayerId, SessionId,
};

use super::{
    AuditRevision, DraftState, ExportRequest, OwnerId, PersistenceError, PersistentDraft,
    ReportPage, ReportPageRequest, Revision, UpdateKey, MAX_REPORT_PAGE_NUMBER,
    MAX_REPORT_PAGE_SIZE,
};

#[test]
fn audit_revisions_match_sqlite_range_and_increment_with_checks() {
    assert_eq!(AuditRevision::default().value(), 0);
    assert_eq!(AuditRevision::new(42).unwrap().next().unwrap().value(), 43);
    let maximum = AuditRevision::new(i64::MAX as u64).unwrap();
    assert_eq!(maximum.next(), Err(PersistenceError::RevisionOverflow));
    assert_eq!(
        AuditRevision::new(u64::MAX),
        Err(PersistenceError::RevisionOverflow)
    );
}

#[test]
fn export_cursors_are_independent_of_report_page_limits() {
    let request = ExportRequest::resume("20.30.20000.30000.20.30.15000.42").unwrap();
    assert!(request.cursor().is_some());
    for cursor in [
        "",
        "1.2.3.4.5.6.7",
        "1.2.3.4.5.6.7.8.9",
        "1.2.10.20.1.2.11.0",
        "1.2.10.20.2.3.0.0",
        "-1.0.0.0.0.0.0.0",
    ] {
        assert_eq!(
            ExportRequest::resume(cursor),
            Err(PersistenceError::InvalidExportCursor)
        );
    }
}

#[test]
fn report_pages_have_bounded_numbers_sizes_and_offsets() {
    let request = ReportPageRequest::new(3, 10).unwrap();
    assert_eq!(request.number(), 3);
    assert_eq!(request.size(), 10);
    assert_eq!(request.offset(), Ok(20));
    for (number, size) in [
        (0, 10),
        (MAX_REPORT_PAGE_NUMBER + 1, 10),
        (1, 0),
        (1, MAX_REPORT_PAGE_SIZE + 1),
    ] {
        assert_eq!(
            ReportPageRequest::new(number, size),
            Err(PersistenceError::InvalidPage)
        );
    }
}

#[test]
fn report_pages_trim_the_lookahead_row_without_repeating_it() {
    let request = ReportPageRequest::new(1, 2).unwrap();
    let page = ReportPage::from_extra(vec![1, 2, 3], request);
    assert_eq!(page.items(), &[1, 2]);
    assert!(page.has_more());
    assert_eq!(page.number(), 1);
}

fn session() -> DraftSession {
    DraftSession::new(
        SessionId::new("draft").unwrap(),
        GameId::new("poker").unwrap(),
        OccurredOn::parse("2026-10-20").unwrap(),
        vec![Participant::Roster(PlayerId::new("one").unwrap())],
    )
    .unwrap()
}

fn draft(state: DraftState, revision: u32) -> PersistentDraft {
    PersistentDraft::new(
        session(),
        OwnerId::new(10).unwrap(),
        vec![],
        Revision::new(revision),
        state,
    )
    .unwrap()
}

#[test]
fn owner_requires_positive_numeric_identity() {
    assert_eq!(OwnerId::new(0), Err(PersistenceError::InvalidOwner));
    assert_eq!(OwnerId::new(-1), Err(PersistenceError::InvalidOwner));
    assert_eq!(OwnerId::new(i64::MAX).unwrap().value(), i64::MAX);
}

#[test]
fn update_key_rejects_empty_and_ambiguous_values() {
    for value in ["", " ", "update/key", "key\n", "é"] {
        assert_eq!(
            UpdateKey::new(value),
            Err(PersistenceError::InvalidUpdateKey)
        );
    }
    assert_eq!(
        UpdateKey::new("telegram_123-4").unwrap().as_str(),
        "telegram_123-4"
    );
}

#[test]
fn revision_increment_is_checked() {
    assert_eq!(Revision::new(0).next(), Ok(Revision::new(1)));
    assert_eq!(
        Revision::new(u32::MAX).next(),
        Err(PersistenceError::RevisionOverflow)
    );
}

#[test]
fn draft_checks_owner_state_and_revision() {
    let open = draft(DraftState::Open, 2);
    assert_eq!(
        open.check_edit(OwnerId::new(11).unwrap(), Revision::new(2)),
        Err(PersistenceError::OwnerMismatch)
    );
    assert_eq!(
        open.check_edit(OwnerId::new(10).unwrap(), Revision::new(1)),
        Err(PersistenceError::RevisionConflict)
    );
    assert!(open
        .check_edit(OwnerId::new(10).unwrap(), Revision::new(2))
        .is_ok());
    assert_eq!(
        draft(DraftState::Consumed, 2).check_edit(OwnerId::new(10).unwrap(), Revision::new(2)),
        Err(PersistenceError::DraftConsumed)
    );
}

#[test]
fn draft_rejects_duplicate_partial_amounts() {
    let entry = AmountEntry::new(
        Participant::Roster(PlayerId::new("one").unwrap()).id(),
        Cents::new(1),
    );
    let result = PersistentDraft::new(
        session(),
        OwnerId::new(10).unwrap(),
        vec![entry.clone(), entry],
        Revision::default(),
        DraftState::Open,
    );
    assert!(matches!(
        result,
        Err(PersistenceError::Invalid(
            crate::domain::DomainError::DuplicateAmount(_)
        ))
    ));
}

#[test]
fn draft_rejects_unselected_partial_amounts() {
    let entry = AmountEntry::new(
        Participant::Roster(PlayerId::new("other").unwrap()).id(),
        Cents::new(1),
    );
    let result = PersistentDraft::new(
        session(),
        OwnerId::new(10).unwrap(),
        vec![entry],
        Revision::default(),
        DraftState::Open,
    );
    assert!(matches!(
        result,
        Err(PersistenceError::Invalid(
            crate::domain::DomainError::UnselectedParticipant(_)
        ))
    ));
}

#[test]
fn domain_still_rejects_guest_owned_by_another_draft() {
    let guest = Guest::new(
        GuestId::new(SessionId::new("other").unwrap(), "guest").unwrap(),
        DisplayName::new("Guest").unwrap(),
    );
    let result = DraftSession::new(
        SessionId::new("draft").unwrap(),
        GameId::new("poker").unwrap(),
        OccurredOn::parse("2026-10-20").unwrap(),
        vec![Participant::Guest(guest)],
    );
    assert!(matches!(
        result,
        Err(crate::domain::DomainError::GuestOutsideSession)
    ));
}

#[test]
fn replay_wins_when_another_request_commits_between_lookup_and_draft_read() {
    let repository = ReplayGap(std::cell::Cell::new(0));
    let command = super::ConfirmCommand {
        update_key: UpdateKey::new("update").unwrap(),
        draft_id: SessionId::new("draft").unwrap(),
        owner: OwnerId::new(10).unwrap(),
        revision: Revision::default(),
        created_at: crate::domain::CreatedAt::parse("2026-10-20T00:00:00Z").unwrap(),
    };
    let result = run_ready(super::Ledger::new(&repository).confirm(&command)).unwrap();
    assert_eq!(result.created_at.to_string(), "2026-10-19T00:00:00Z");
    assert_eq!(repository.0.get(), 2);
}

// A unit test of the injected use case's lookup race, not a D1 integration fake.
struct ReplayGap(std::cell::Cell<u8>);

impl super::DraftRepository for ReplayGap {
    async fn create_draft(&self, _: &PersistentDraft) -> super::PersistenceResult<()> {
        panic!("replay must not create a draft")
    }

    async fn replace_draft(
        &self,
        _: &PersistentDraft,
        _: Revision,
    ) -> super::PersistenceResult<()> {
        panic!("replay must not edit a draft")
    }

    async fn draft(&self, _: &SessionId) -> super::PersistenceResult<Option<PersistentDraft>> {
        Ok(Some(draft(DraftState::Consumed, 1)))
    }
}

impl super::UpdateRepository for ReplayGap {
    async fn processed_update(
        &self,
        _: &UpdateKey,
    ) -> super::PersistenceResult<Option<super::Confirmation>> {
        self.0.set(self.0.get() + 1);
        if self.0.get() == 1 {
            return Ok(None);
        }
        Ok(Some(super::Confirmation {
            session_id: SessionId::new("draft").unwrap(),
            owner: OwnerId::new(10).unwrap(),
            created_at: crate::domain::CreatedAt::parse("2026-10-19T00:00:00Z").unwrap(),
        }))
    }
}

impl super::ConfirmationRepository for ReplayGap {
    async fn commit_confirmation(
        &self,
        _: &UpdateKey,
        _: OwnerId,
        _: Revision,
        _: &crate::domain::ConfirmedSession,
    ) -> super::PersistenceResult<super::Confirmation> {
        panic!("replay must not write a second session")
    }
}

fn run_ready<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    match future.as_mut().poll(&mut context) {
        std::task::Poll::Ready(value) => value,
        std::task::Poll::Pending => panic!("unit repository futures must resolve immediately"),
    }
}
