use super::{Error, Result};
use crate::persistence::{OwnerId, UpdateKey};
use serde::Deserialize;

pub const SECRET_HEADER: &str = "x-telegram-bot-api-secret-token";
const MAX_SECRET_BYTES: usize = 256;
const MAX_BOT_USERNAME_BYTES: usize = 32;

pub struct Configuration {
    secret: String,
    bot_username: String,
    chat: i64,
    administrators: Vec<OwnerId>,
}

impl Configuration {
    pub fn new(
        secret: String,
        bot_username: String,
        chat: i64,
        administrators: Vec<i64>,
    ) -> Result<Self> {
        validate_secret(&secret)?;
        validate_bot_username(&bot_username)?;
        if chat >= 0 || administrators.is_empty() {
            return Err(Error::Configuration);
        }
        let administrators = administrators
            .into_iter()
            .map(OwnerId::new)
            .collect::<std::result::Result<_, _>>()
            .map_err(|_| Error::Configuration)?;
        Ok(Self {
            secret,
            bot_username,
            chat,
            administrators,
        })
    }

    pub(super) fn bot_username(&self) -> &str {
        &self.bot_username
    }

    pub fn check_secret(&self, supplied: Option<&str>) -> Result<()> {
        let supplied = supplied.ok_or(Error::Unauthorized)?.as_bytes();
        let mismatch = self
            .secret
            .as_bytes()
            .iter()
            .zip(supplied)
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            });
        if supplied.len() != self.secret.len() || mismatch != 0 {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }

    fn authorize(&self, chat: &Chat, user: &User) -> Result<OwnerId> {
        let owner = OwnerId::new(user.id).map_err(|_| Error::Rejected)?;
        if chat.id != self.chat
            || !matches!(chat.kind.as_str(), "group" | "supergroup")
            || user.is_bot
            || !self.administrators.contains(&owner)
        {
            return Err(Error::Rejected);
        }
        Ok(owner)
    }
}

fn validate_bot_username(username: &str) -> Result<()> {
    let valid = username
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    if username.is_empty() || username.len() > MAX_BOT_USERNAME_BYTES || !valid {
        return Err(Error::Configuration);
    }
    Ok(())
}

fn validate_secret(secret: &str) -> Result<()> {
    let valid = secret
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte));
    if secret.is_empty() || secret.len() > MAX_SECRET_BYTES || !valid {
        return Err(Error::Configuration);
    }
    Ok(())
}

#[derive(Deserialize)]
struct Update {
    update_id: u64,
    message: Option<Message>,
    callback_query: Option<Callback>,
}
#[derive(Deserialize)]
struct Chat {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
}
#[derive(Deserialize)]
struct User {
    id: i64,
    is_bot: bool,
}
#[derive(Deserialize)]
struct Message {
    chat: Chat,
    from: Option<User>,
    sender_chat: Option<serde_json::Value>,
    business_connection_id: Option<String>,
    text: Option<String>,
    date: i64,
}
#[derive(Deserialize)]
struct Callback {
    id: String,
    from: User,
    message: Option<Message>,
    data: Option<String>,
}

pub(super) struct AuthorizedUpdate {
    pub key: UpdateKey,
    pub number: u64,
    pub owner: OwnerId,
    pub chat: i64,
    pub input: String,
    pub callback: bool,
    pub callback_id: Option<String>,
}

impl AuthorizedUpdate {
    pub fn fingerprint(&self) -> String {
        serde_json::json!([
            self.chat.to_string(),
            self.owner.value().to_string(),
            self.callback,
            self.input
        ])
        .to_string()
    }
}

pub(super) fn decode(config: &Configuration, body: &[u8]) -> Result<AuthorizedUpdate> {
    let update: Update = serde_json::from_slice(body).map_err(|_| Error::Invalid)?;
    let callback_id = update.callback_query.as_ref().map(|query| query.id.clone());
    let (message, user, input, callback) = extract(update.message, update.callback_query)?;
    if message.sender_chat.is_some()
        || message.business_connection_id.is_some()
        || message.date <= 0
    {
        return Err(Error::Rejected);
    }
    let owner = config.authorize(&message.chat, &user)?;
    super::policy::input(&input, callback)?;
    if callback_id
        .as_ref()
        .is_some_and(|id| id.is_empty() || id.len() > super::policy::MAX_CALLBACK_ID_BYTES)
    {
        return Err(Error::Limit);
    }
    Ok(AuthorizedUpdate {
        key: UpdateKey::new(&format!("telegram_{}", update.update_id))?,
        number: update.update_id,
        owner,
        chat: message.chat.id,
        input,
        callback,
        callback_id,
    })
}

fn extract(
    message: Option<Message>,
    callback: Option<Callback>,
) -> Result<(Message, User, String, bool)> {
    match (message, callback) {
        (Some(message), None) => extract_message(message),
        (None, Some(callback)) => extract_callback(callback),
        _ => Err(Error::Rejected),
    }
}

fn extract_message(mut message: Message) -> Result<(Message, User, String, bool)> {
    let user = message.from.take().ok_or(Error::Rejected)?;
    let text = message.text.take().ok_or(Error::Invalid)?;
    Ok((message, user, text, false))
}

fn extract_callback(callback: Callback) -> Result<(Message, User, String, bool)> {
    if callback.id.is_empty() {
        return Err(Error::Rejected);
    }
    let message = callback.message.ok_or(Error::Rejected)?;
    Ok((
        message,
        callback.from,
        callback.data.ok_or(Error::Invalid)?,
        true,
    ))
}
