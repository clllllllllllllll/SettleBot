use super::Error;

#[derive(Debug, serde::Serialize)]
pub(crate) struct Event {
    event: &'static str,
    category: &'static str,
    retryable: bool,
}

pub(crate) fn event(error: &Error) -> Event {
    let (category, retryable) = match error {
        Error::Unauthorized | Error::Rejected => ("authorization", false),
        Error::Invalid | Error::Stale | Error::Limit => ("input", false),
        Error::RateLimited => ("throttled", false),
        Error::Timeout | Error::Telegram => ("telegram", true),
        Error::Storage => ("storage", true),
        Error::Configuration => ("configuration", true),
    };
    Event {
        event: "telegram_webhook_failed",
        category,
        retryable,
    }
}

pub(crate) fn record(error: &Error) {
    let value = serde_json::to_string(&event(error))
        .unwrap_or_else(|_| r#"{"event":"telegram_webhook_failed"}"#.into());
    worker::console_log!("{}", value);
}
