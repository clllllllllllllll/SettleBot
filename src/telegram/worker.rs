//! Workers adapter. Sends are injected; callback acknowledgements use webhook replies.
use super::{
    auth::SECRET_HEADER, deliver, render::Reply, Clock, Configuration, Error, Result, Transport,
};
use crate::{
    domain::CreatedAt,
    persistence::d1::{D1Repository, DATABASE_BINDING},
};
use http::{header::CONTENT_TYPE, Response, StatusCode};
use worker::{Env, HttpRequest};

pub(crate) const WEBHOOK_PATH: &str = "/telegram/webhook";
const WEBHOOK_SECRET: &str = "TELEGRAM_WEBHOOK_SECRET";
const CHAT_ID: &str = "TELEGRAM_CHAT_ID";
const ADMIN_IDS: &str = "TELEGRAM_ADMIN_IDS";
const BOT_USERNAME: &str = "TELEGRAM_BOT_USERNAME";
const BOT_TOKEN: &str = "TELEGRAM_BOT_TOKEN";
const API_ORIGIN: &str = "https://api.telegram.org";

struct WorkerClock;
impl Clock for WorkerClock {
    fn now(&self) -> Result<CreatedAt> {
        let milliseconds = worker::Date::now().as_millis();
        let milliseconds = i64::try_from(milliseconds).map_err(|_| Error::Storage)?;
        let instant =
            chrono::DateTime::from_timestamp_millis(milliseconds).ok_or(Error::Storage)?;
        Ok(CreatedAt::new(instant))
    }
}

pub(crate) async fn webhook(
    request: HttpRequest,
    environment: Env,
) -> worker::Result<Response<String>> {
    response(process(request, environment).await)
}

pub(crate) fn response(result: Result<String>) -> worker::Result<Response<String>> {
    let (status, body) = match result {
        Ok(body) => (StatusCode::OK, body),
        Err(Error::Unauthorized) => (StatusCode::FORBIDDEN, "{\"error\":\"Forbidden\"}".into()),
        Err(Error::Rejected | Error::Invalid | Error::Stale) => {
            (StatusCode::OK, "{\"ok\":true}".into())
        }
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "{\"error\":\"Unavailable\"}".into(),
        ),
    };
    Ok(Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "application/json")
        .body(body)?)
}

async fn process(request: HttpRequest, environment: Env) -> Result<String> {
    let config = configuration(&environment)?;
    let secret = request
        .headers()
        .get(SECRET_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    config.check_secret(secret.as_deref())?;
    let mut request: worker::Request = request.try_into().map_err(|_| Error::Invalid)?;
    let body = request.bytes().await.map_err(|_| Error::Invalid)?;
    process_body(&config, secret.as_deref(), &body, &environment).await
}

async fn process_body(
    config: &Configuration,
    secret: Option<&str>,
    body: &[u8],
    environment: &Env,
) -> Result<String> {
    let repository = D1Repository::new(
        environment
            .d1(DATABASE_BINDING)
            .map_err(|_| Error::Configuration)?,
    );
    let transport = BotApi {
        token: secret_value(environment, BOT_TOKEN)?,
    };
    let reply = deliver(config, secret, body, &repository, &WorkerClock, &transport).await?;
    serde_json::to_string(&reply).map_err(|_| Error::Storage)
}

fn configuration(environment: &Env) -> Result<Configuration> {
    let secret = secret_value(environment, WEBHOOK_SECRET)?;
    let bot_username = secret_value(environment, BOT_USERNAME)?;
    let chat = secret_value(environment, CHAT_ID)?
        .parse()
        .map_err(|_| Error::Configuration)?;
    let admins = secret_value(environment, ADMIN_IDS)?
        .split(',')
        .map(|id| id.trim().parse::<i64>())
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| Error::Configuration)?;
    Configuration::new(secret, bot_username, chat, admins)
}

fn secret_value(environment: &Env, name: &str) -> Result<String> {
    environment
        .secret(name)
        .map(|value| value.to_string())
        .map_err(|_| Error::Configuration)
}

struct BotApi {
    token: String,
}

impl Transport for BotApi {
    async fn send(&self, reply: &Reply) -> Result<()> {
        let request = self.request(reply)?;
        let mut response = worker::Fetch::Request(request)
            .send()
            .await
            .map_err(|_| Error::Storage)?;
        let status = response.status_code();
        let result: ApiResponse = response.json().await.map_err(|_| Error::Storage)?;
        if status != StatusCode::OK.as_u16() || !result.ok {
            return Err(Error::Storage);
        }
        Ok(())
    }
}

impl BotApi {
    fn request(&self, reply: &Reply) -> Result<worker::Request> {
        let body = serde_json::to_string(reply).map_err(|_| Error::Storage)?;
        let headers = worker::Headers::new();
        headers
            .set("Content-Type", "application/json")
            .map_err(|_| Error::Storage)?;
        let mut init = worker::RequestInit::new();
        init.with_method(worker::Method::Post)
            .with_headers(headers)
            .with_body(Some(body.into()));
        let url = format!("{API_ORIGIN}/bot{}/sendMessage", self.token);
        worker::Request::new_with_init(&url, &init).map_err(|_| Error::Configuration)
    }
}

#[derive(serde::Deserialize)]
struct ApiResponse {
    ok: bool,
}
