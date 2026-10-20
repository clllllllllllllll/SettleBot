use std::fmt;

use chrono::{DateTime, Datelike, NaiveDate, SecondsFormat, Utc};

use super::DomainError;

const DATE_FORMAT: &str = "%Y-%m-%d";
const CALENDAR_YEAR_RANGE: std::ops::RangeInclusive<i32> = 1..=9999;

/// The explicitly selected Asia/Singapore calendar date, independent of entry time.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct OccurredOn(NaiveDate);

impl OccurredOn {
    pub fn new(year: i32, month: u32, day: u32) -> Result<Self, DomainError> {
        if !CALENDAR_YEAR_RANGE.contains(&year) {
            return Err(DomainError::InvalidOccurredOn);
        }
        NaiveDate::from_ymd_opt(year, month, day)
            .map(Self)
            .ok_or(DomainError::InvalidOccurredOn)
    }

    pub fn parse(value: &str) -> Result<Self, DomainError> {
        if value.trim().is_empty() {
            return Err(DomainError::RequiredValue("occurred_on"));
        }
        let date = NaiveDate::parse_from_str(value, DATE_FORMAT)
            .map_err(|_| DomainError::InvalidOccurredOn)?;
        let date = Self::new(date.year(), date.month(), date.day())?;
        if date.to_string() != value {
            return Err(DomainError::InvalidOccurredOn);
        }
        Ok(date)
    }
}

impl fmt::Display for OccurredOn {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0.format(DATE_FORMAT))
    }
}

/// An injected UTC instant. This type never reads a clock or supplies a played date.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CreatedAt(DateTime<Utc>);

impl CreatedAt {
    pub fn new(instant: DateTime<Utc>) -> Self {
        Self(instant)
    }

    pub fn parse(value: &str) -> Result<Self, DomainError> {
        if value.trim().is_empty() {
            return Err(DomainError::RequiredValue("created_at"));
        }
        let instant =
            DateTime::parse_from_rfc3339(value).map_err(|_| DomainError::InvalidCreatedAt)?;
        if instant.offset().local_minus_utc() != 0 {
            return Err(DomainError::InvalidCreatedAt);
        }
        Ok(Self(instant.with_timezone(&Utc)))
    }

    pub fn value(self) -> DateTime<Utc> {
        self.0
    }
}

impl fmt::Display for CreatedAt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0.to_rfc3339_opts(SecondsFormat::AutoSi, true))
    }
}
