use std::fmt;

use super::{PersistenceError, PersistenceResult};
use crate::domain::Cents;

const CENTS_PER_DOLLAR: u128 = 100;

/// Exact report cents; individual stored entries retain their signed i64 range.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct ReportTotal(i128);

impl ReportTotal {
    pub const fn new(value: i128) -> Self {
        Self(value)
    }

    pub const fn value(self) -> i128 {
        self.0
    }

    pub fn add(&mut self, amount: Cents) -> PersistenceResult<()> {
        self.0 = self
            .0
            .checked_add(i128::from(amount.value()))
            .ok_or(PersistenceError::Corrupt("report aggregate overflow"))?;
        Ok(())
    }
}

impl fmt::Display for ReportTotal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let magnitude = self.0.unsigned_abs();
        let dollars = magnitude / CENTS_PER_DOLLAR;
        let cents = magnitude % CENTS_PER_DOLLAR;
        write!(formatter, "{sign}S${dollars}.{cents:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_format_widened_boundaries_exactly_and_check_arithmetic() {
        for (value, display) in [
            (0, "S$0.00"),
            (-1, "-S$0.01"),
            (i128::from(i64::MAX) + 2, "S$92233720368547758.09"),
            (i128::from(i64::MIN) - 1, "-S$92233720368547758.09"),
            (i128::MAX, "S$1701411834604692317316873037158841057.27"),
            (i128::MIN, "-S$1701411834604692317316873037158841057.28"),
        ] {
            assert_eq!(ReportTotal::new(value).to_string(), display);
        }
        let mut maximum = ReportTotal::new(i128::MAX);
        assert!(maximum.add(Cents::new(1)).is_err());
        assert_eq!(maximum.value(), i128::MAX);
    }
}
