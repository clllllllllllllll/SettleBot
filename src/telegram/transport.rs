use super::{auth::decode, handle, render::Reply, Clock, Configuration, Error, Repository, Result};
use serde_json::{json, Value};

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
    let reply = match handle(config, secret, body, repository, clock).await {
        Ok(reply) => reply,
        Err(Error::Invalid | Error::Stale) => Reply::text(update.chat, recovery(&update.input)),
        Err(error) => return Err(error),
    };
    transport.send(&reply).await?;
    match update.callback_id {
        Some(id) => Ok(json!({"method": "answerCallbackQuery", "callback_query_id": id})),
        None => Ok(json!({"ok": true})),
    }
}

fn recovery(input: &str) -> String {
    let candidate = input.split(':').next().unwrap_or_default();
    if candidate.starts_with('t') && candidate[1..].bytes().all(|b| b.is_ascii_digit()) {
        return format!("That step is invalid or out of date. Resume with /resume {candidate}.");
    }
    "Invalid or out-of-date input. Use the current prompt, /resume SESSION_ID, or /session. Add a permanent player with /player Name or a game with /game Name. Amounts need a sign and two decimals, such as -12.50.".into()
}
