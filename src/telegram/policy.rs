use super::{Error, Result};

pub(crate) const MAX_WEBHOOK_BYTES: usize = 16 * 1024;
pub(crate) const MAX_TEXT_BYTES: usize = 512;
pub(crate) const MAX_CALLBACK_BYTES: usize = 64;
pub(crate) const MAX_CALLBACK_ID_BYTES: usize = 128;
pub(crate) const MAX_CATALOG_CHOICES: usize = 24;
pub(crate) const MAX_REQUESTS_PER_WINDOW: u32 = 20;
pub(crate) const RATE_WINDOW_SECONDS: i64 = 60;
pub(crate) const SAFE_RETRY_ATTEMPTS: usize = 2;
pub(crate) const TELEGRAM_TIMEOUT_SECONDS: u64 = 5;
pub const MAX_PAGE_SIZE: u8 = 50;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageSize(u8);

impl PageSize {
    pub fn new(value: u8) -> Result<Self> {
        if value == 0 || value > MAX_PAGE_SIZE {
            return Err(Error::Limit);
        }
        Ok(Self(value))
    }

    pub fn value(self) -> u8 {
        self.0
    }
}

pub(crate) fn input(value: &str, callback: bool) -> Result<()> {
    let limit = if callback {
        MAX_CALLBACK_BYTES
    } else {
        MAX_TEXT_BYTES
    };
    if value.len() > limit {
        return Err(Error::Limit);
    }
    Ok(())
}

pub(crate) fn catalog_capacity(current: usize) -> Result<()> {
    if current >= MAX_CATALOG_CHOICES {
        return Err(Error::Limit);
    }
    Ok(())
}
