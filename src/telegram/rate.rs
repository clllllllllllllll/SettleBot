use super::{Error, Result};
use crate::{
    domain::CreatedAt,
    persistence::{OwnerId, UpdateKey},
};

#[derive(Clone)]
pub(crate) struct RateLimitRequest {
    pub key: UpdateKey,
    pub owner: OwnerId,
    pub now: CreatedAt,
}

#[allow(async_fn_in_trait)]
pub(crate) trait RateLimiter {
    async fn admit(&self, request: &RateLimitRequest) -> Result<bool>;
}

pub(crate) async fn admit_with_retry(
    limiter: &impl RateLimiter,
    request: &RateLimitRequest,
) -> Result<()> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        match limiter.admit(request).await {
            Ok(true) => return Ok(()),
            Ok(false) => return Err(Error::RateLimited),
            Err(Error::Storage) if attempts < super::policy::SAFE_RETRY_ATTEMPTS => {}
            Err(error) => return Err(error),
        }
    }
}
