//! Conservative sendMessage response classification. Never expose API descriptions in logs.
use super::{Error, Result};
use serde::Deserialize;

#[derive(Deserialize)]
struct ApiResponse {
    ok: bool,
    error_code: Option<u16>,
    description: Option<String>,
    result: Option<serde_json::Value>,
    parameters: Option<ResponseParameters>,
}

#[derive(Default, Deserialize)]
struct ResponseParameters {
    retry_after: Option<u64>,
    migrate_to_chat_id: Option<i64>,
}

pub(crate) fn classify(status: u16, body: &[u8]) -> Result<()> {
    if status >= 500 {
        return Err(Error::TelegramServer);
    }
    let response: ApiResponse =
        serde_json::from_slice(body).map_err(|_| Error::TelegramAmbiguous)?;
    if status == 200 && response.ok && response.result.is_some() {
        return Ok(());
    }
    if response.ok || response.error_code != Some(status) {
        return Err(Error::TelegramAmbiguous);
    }
    failure(status, response)
}

fn failure(status: u16, response: ApiResponse) -> Result<()> {
    let parameters = response.parameters.unwrap_or_default();
    if status == 429 || parameters.retry_after.is_some() {
        return Err(Error::TelegramRateLimited {
            retry_after: parameters.retry_after,
        });
    }
    if matches!(status, 401 | 403 | 404) || parameters.migrate_to_chat_id.is_some() {
        return Err(Error::TelegramAccess);
    }
    if status == 400 && permanent_message_rejection(response.description.as_deref()) {
        return Err(Error::TelegramRejected);
    }
    Err(Error::TelegramAmbiguous)
}

fn permanent_message_rejection(description: Option<&str>) -> bool {
    matches!(
        description,
        Some("Bad Request: message is too long" | "Bad Request: message text is empty")
    ) || description.is_some_and(|value| value.starts_with("Bad Request: can't parse entities:"))
}
