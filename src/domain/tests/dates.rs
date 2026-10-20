use chrono::{DateTime, FixedOffset};

use super::*;

#[test]
fn played_date_requires_an_explicit_value() {
    for value in ["", " ", "\n"] {
        assert_eq!(
            OccurredOn::parse(value),
            Err(DomainError::RequiredValue("occurred_on"))
        );
    }
}

#[test]
fn played_date_rejects_invalid_calendar_values() {
    for value in [
        "2026-02-29",
        "2024-02-30",
        "2026-00-10",
        "2026-13-10",
        "2026-10-00",
        "2026-04-31",
        "0000-01-01",
        "10000-01-01",
    ] {
        assert_eq!(
            OccurredOn::parse(value),
            Err(DomainError::InvalidOccurredOn)
        );
    }
}

#[test]
fn played_date_rejects_noncanonical_values() {
    for value in [
        "2026-1-1",
        " 2026-10-20",
        "2026-10-20T00:00:00Z",
        "not-a-date",
    ] {
        assert_eq!(
            OccurredOn::parse(value),
            Err(DomainError::InvalidOccurredOn)
        );
    }
}

#[test]
fn played_date_validates_leap_years_and_calendar_boundaries() -> Result<(), DomainError> {
    for value in ["0001-01-01", "2000-02-29", "2024-02-29", "9999-12-31"] {
        assert_eq!(OccurredOn::parse(value)?.to_string(), value);
    }
    for (year, month, day) in [(1900, 2, 29), (2026, 13, 1), (0, 1, 1), (10000, 1, 1)] {
        assert_eq!(
            OccurredOn::new(year, month, day),
            Err(DomainError::InvalidOccurredOn)
        );
    }
    Ok(())
}

#[test]
fn creation_instant_requires_an_explicit_value() {
    for value in ["", " ", "\n"] {
        assert_eq!(
            CreatedAt::parse(value),
            Err(DomainError::RequiredValue("created_at"))
        );
    }
}

#[test]
fn creation_instant_rejects_invalid_or_non_utc_values() {
    for value in [
        "2026-02-29T00:00:00Z",
        "2026-10-20T24:00:00Z",
        "2026-10-20",
        "2026-10-20T00:00:00",
        "2026-10-20T08:00:00+08:00",
        "not-an-instant",
    ] {
        assert_eq!(CreatedAt::parse(value), Err(DomainError::InvalidCreatedAt));
    }
}

#[test]
fn utc_instants_preserve_injected_precision() -> Result<(), DomainError> {
    let instant = CreatedAt::parse("2026-10-20T16:00:00.123456789Z")?;
    assert_eq!(CreatedAt::new(instant.value()), instant);
    assert_eq!(instant.to_string(), "2026-10-20T16:00:00.123456789Z");
    assert_eq!(CreatedAt::parse("2026-10-20T16:00:00+00:00")?, created_at());
    Ok(())
}

#[test]
fn singapore_midnight_never_replaces_the_selected_played_date() -> Result<(), DomainError> {
    let cases = [
        ("2026-10-20T15:59:59Z", "2026-10-20"),
        ("2026-10-20T16:00:00Z", "2026-10-21"),
        ("2026-10-20T16:00:01Z", "2026-10-21"),
    ];
    for (utc, singapore_date) in cases {
        let session = confirm_at(CreatedAt::parse(utc)?)?;
        assert_eq!(session.occurred_on().to_string(), "2026-10-18");
        assert_eq!(session.created_at().to_string(), utc);
        assert_singapore_date(session.created_at().value(), singapore_date);
    }
    Ok(())
}

fn confirm_at(instant: CreatedAt) -> Result<ConfirmedSession, DomainError> {
    let participants = vec![roster("player-a"), roster("player-b")];
    let entries = participants
        .iter()
        .map(|participant| entry(participant, 0))
        .collect();
    draft(participants)?.confirm(entries, instant)
}

fn assert_singapore_date(instant: DateTime<chrono::Utc>, expected: &str) {
    const SINGAPORE_OFFSET_SECONDS: i32 = 8 * 60 * 60;
    let singapore = FixedOffset::east_opt(SINGAPORE_OFFSET_SECONDS).expect("valid fixture offset");
    assert_eq!(
        instant.with_timezone(&singapore).date_naive().to_string(),
        expected
    );
}
