use std::fmt;

use super::DomainError;

const CENTS_PER_DOLLAR: u64 = 100;

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Cents(i64);

impl Cents {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> i64 {
        self.0
    }

    /// Sum widely before narrowing, so participant ordering cannot cause overflow.
    pub fn checked_sum(amounts: impl IntoIterator<Item = Self>) -> Result<Self, DomainError> {
        let total = amounts.into_iter().try_fold(0_i128, |total, amount| {
            total.checked_add(i128::from(amount.0))
        });
        let total = total.ok_or(DomainError::AmountOverflow)?;
        i64::try_from(total)
            .map(Self)
            .map_err(|_| DomainError::AmountOverflow)
    }
}

impl fmt::Display for Cents {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let magnitude = self.0.unsigned_abs();
        let dollars = magnitude / CENTS_PER_DOLLAR;
        let cents = magnitude % CENTS_PER_DOLLAR;
        write!(formatter, "{sign}S${dollars}.{cents:02}")
    }
}
