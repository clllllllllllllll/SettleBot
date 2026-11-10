use super::{auth::decode, handle, render::Reply, Clock, Configuration, Error, Repository, Result};
use serde_json::{json, Value};

pub(crate) async fn receive(
    config: &Configuration,
    secret: Option<&str>,
    body: &[u8],
    repository: &impl Repository,
    clock: &impl Clock,
    transport: &impl Transport,
    limiter: &impl super::RateLimiter,
) -> Result<Value> {
    config.check_secret(secret)?;
    if !super::worker::admit_update(config, body, limiter, clock).await? {
        return Ok(json!({"ok": true}));
    }
    deliver(config, secret, body, repository, clock, transport).await
}

#[allow(async_fn_in_trait)]
pub(crate) trait Transport {
    async fn send(&self, reply: &Reply) -> Result<()>;
}

pub(crate) async fn deliver(
    config: &Configuration,
    secret: Option<&str>,
    body: &[u8],
    repository: &impl Repository,
    clock: &impl Clock,
    transport: &impl Transport,
) -> Result<Value> {
    config.check_secret(secret)?;
    let update = decode(config, body)?;
    if super::commands::classify(&update, config.bot_username())?
        == super::commands::InputKind::Ignore
    {
        return Ok(json!({"ok": true}));
    }
    let reply = match handle(config, secret, body, repository, clock).await {
        Ok(reply) => reply,
        Err(Error::Invalid | Error::Stale) => Reply::text(update.chat, recovery(&update.input)),
        Err(Error::Limit) => Reply::text(update.chat, limit_message()),
        Err(error) => return Err(error),
    };
    match transport.send(&reply).await {
        Ok(()) => {}
        Err(Error::TelegramRejected) => super::logging::record(&Error::TelegramRejected),
        Err(error) => return Err(error),
    }
    match update.callback_id {
        Some(id) => Ok(json!({"method": "answerCallbackQuery", "callback_query_id": id})),
        None => Ok(json!({"ok": true})),
    }
}

fn limit_message() -> String {
    "That input or selection is too large. Shorten it or select fewer participants and try again."
        .into()
}

fn recovery(input: &str) -> String {
    let candidate = input.split(':').next().unwrap_or_default();
    if candidate.starts_with('t') && candidate[1..].bytes().all(|b| b.is_ascii_digit()) {
        return format!("That step is invalid or out of date. Resume with /resume {candidate}.");
    }
    "Invalid or out-of-date input. Use the current prompt, /resume SESSION_ID, or /session. Add a permanent player with /player Name or a game with /game Name. Amounts need a sign and two decimals, such as -12.50.".into()
}
