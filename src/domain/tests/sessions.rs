use super::*;

#[test]
fn confirmation_transitions_a_balanced_draft_to_one_game() -> Result<(), DomainError> {
    let participants = vec![roster("player-a"), roster("player-b")];
    let entries = vec![
        entry(&participants[0], 12345),
        entry(&participants[1], -12345),
    ];
    let draft = draft(participants.clone())?;
    assert_eq!(draft.state(), SessionState::Draft);
    let confirmed = draft.confirm(entries.clone(), created_at())?;
    assert_eq!(confirmed.state(), SessionState::Confirmed);
    assert_eq!(confirmed.id(), &session_id());
    assert_eq!(confirmed.game_id(), &game_id());
    assert_eq!(confirmed.participants(), participants);
    assert_eq!(confirmed.entries(), entries);
    Ok(())
}

#[test]
fn duplicate_roster_participants_are_rejected() {
    let participant = roster("player-a");
    assert_eq!(
        draft(vec![participant.clone(), participant.clone()]),
        Err(DomainError::DuplicateParticipant(participant.id()))
    );
}

#[test]
fn duplicate_guests_are_rejected_by_identity_despite_different_names() {
    let first = Participant::Guest(guest(session_id(), "visitor-1", "Visitor A"));
    let renamed = Participant::Guest(guest(session_id(), "visitor-1", "Visitor B"));
    assert_eq!(
        draft(vec![first.clone(), renamed]),
        Err(DomainError::DuplicateParticipant(first.id()))
    );
}

#[test]
fn guests_cannot_be_selected_outside_their_own_session() -> Result<(), DomainError> {
    let other_session = SessionId::new("session-2")?;
    let participant = Participant::Guest(guest(other_session, "visitor-1", "Visitor A"));
    assert_eq!(
        draft(vec![roster("player-a"), participant]),
        Err(DomainError::GuestOutsideSession)
    );
    Ok(())
}

#[test]
fn fewer_than_two_participants_cannot_be_confirmed() -> Result<(), DomainError> {
    for participants in [vec![], vec![roster("player-a")]] {
        assert_eq!(
            draft(participants)?.confirm(vec![], created_at()),
            Err(DomainError::TooFewParticipants)
        );
    }
    Ok(())
}

#[test]
fn every_selected_participant_requires_one_amount() -> Result<(), DomainError> {
    let participants = vec![roster("player-a"), roster("player-b")];
    let missing = participants[1].id();
    let entries = vec![entry(&participants[0], 0)];
    assert_eq!(
        draft(participants)?.confirm(entries, created_at()),
        Err(DomainError::MissingAmount(missing))
    );
    Ok(())
}

#[test]
fn an_empty_amount_collection_cannot_confirm_a_selection() -> Result<(), DomainError> {
    let participants = vec![roster("player-a"), roster("player-b")];
    let first_missing = participants[0].id();
    assert_eq!(
        draft(participants)?.confirm(vec![], created_at()),
        Err(DomainError::MissingAmount(first_missing))
    );
    Ok(())
}

#[test]
fn duplicate_amounts_cannot_replace_a_missing_amount() -> Result<(), DomainError> {
    let participants = vec![roster("player-a"), roster("player-b")];
    let duplicate = participants[0].id();
    let entries = vec![entry(&participants[0], 100), entry(&participants[0], -100)];
    assert_eq!(
        draft(participants)?.confirm(entries, created_at()),
        Err(DomainError::DuplicateAmount(duplicate))
    );
    Ok(())
}

#[test]
fn an_unselected_player_cannot_receive_an_amount() -> Result<(), DomainError> {
    let participants = vec![roster("player-a"), roster("player-b")];
    let unselected = roster("player-c");
    let entries = vec![entry(&participants[0], 100), entry(&unselected, -100)];
    assert_eq!(
        draft(participants)?.confirm(entries, created_at()),
        Err(DomainError::UnselectedParticipant(unselected.id()))
    );
    Ok(())
}

#[test]
fn extra_amounts_are_rejected_even_when_the_total_is_zero() -> Result<(), DomainError> {
    let participants = vec![roster("player-a"), roster("player-b")];
    let unselected = roster("player-c");
    let entries = vec![
        entry(&participants[0], 100),
        entry(&participants[1], -100),
        entry(&unselected, 0),
    ];
    assert_eq!(
        draft(participants)?.confirm(entries, created_at()),
        Err(DomainError::UnselectedParticipant(unselected.id()))
    );
    Ok(())
}

#[test]
fn nonzero_totals_are_rejected() {
    for (amounts, total) in [([100, -99], 1), ([100, -101], -1)] {
        assert_eq!(
            confirm_roster(&amounts),
            Err(DomainError::NonZeroTotal(Cents::new(total)))
        );
    }
}

#[test]
fn confirmation_rejects_totals_outside_signed_cents() {
    for amounts in [[i64::MAX, 1], [i64::MIN, -1]] {
        assert_eq!(confirm_roster(&amounts), Err(DomainError::AmountOverflow));
    }
}

#[test]
fn confirmation_handles_boundary_values_in_any_order() -> Result<(), DomainError> {
    for amounts in [[i64::MAX, 1, i64::MIN], [i64::MIN, 1, i64::MAX]] {
        assert_eq!(confirm_roster(&amounts)?.entries().len(), 3);
    }
    assert_eq!(confirm_roster(&[0, 0])?.entries().len(), 2);
    Ok(())
}

#[test]
fn guest_and_roster_with_the_same_local_id_remain_distinct() -> Result<(), DomainError> {
    let roster = roster("shared-id");
    let guest = Participant::Guest(guest(session_id(), "shared-id", "Visitor A"));
    assert_ne!(roster.id(), guest.id());
    let entries = vec![entry(&roster, -750), entry(&guest, 750)];
    let confirmed = draft(vec![roster.clone(), guest.clone()])?.confirm(entries, created_at())?;
    assert_eq!(confirmed.participants(), [roster, guest]);
    assert_eq!(
        Cents::checked_sum(confirmed.entries().iter().map(AmountEntry::amount))?,
        Cents::ZERO
    );
    Ok(())
}

#[test]
fn guest_names_do_not_control_identity_or_create_roster_members() -> Result<(), DomainError> {
    let first = Participant::Guest(guest(session_id(), "visitor-1", "Visitor A"));
    let second = Participant::Guest(guest(session_id(), "visitor-2", "Visitor A"));
    let entries = vec![entry(&first, -500), entry(&second, 500)];
    let confirmed = draft(vec![first, second])?.confirm(entries, created_at())?;
    assert!(confirmed
        .participants()
        .iter()
        .all(|participant| matches!(participant, Participant::Guest(_))));
    Ok(())
}

#[test]
fn unselected_guests_and_guests_from_other_sessions_have_no_entry() -> Result<(), DomainError> {
    for owner in [session_id(), SessionId::new("session-2")?] {
        let participants = vec![roster("player-a"), roster("player-b")];
        let unselected = Participant::Guest(guest(owner, "visitor-1", "Visitor A"));
        let entries = vec![entry(&participants[0], -100), entry(&unselected, 100)];
        assert_eq!(
            draft(participants)?.confirm(entries, created_at()),
            Err(DomainError::UnselectedParticipant(unselected.id()))
        );
    }
    Ok(())
}
