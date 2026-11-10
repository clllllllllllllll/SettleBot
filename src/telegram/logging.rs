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
        Error::Invalid | Error::Stale | Error::AuditStale | Error::Limit => ("input", false),
        Error::RateLimited => ("throttled", false),
        Error::ReportCapacity => ("report_capacity", false),
        Error::Timeout => ("telegram_timeout", true),
        Error::TelegramNetwork => ("telegram_network", true),
        Error::TelegramAmbiguous => ("telegram_ambiguous", true),
        Error::TelegramRateLimited { .. } => ("telegram_rate_limit", true),
        Error::TelegramServer => ("telegram_server", true),
        Error::TelegramAccess => ("telegram_access", true),
        Error::TelegramRejected => ("telegram_rejection", false),
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
