//! Test-only injection boundary; never sends a request to Telegram.
use crate::telegram::{self, Clock, Configuration, Error, Result, Transport};
use crate::{
    domain::CreatedAt,
    persistence::d1::{D1Repository, DATABASE_BINDING},
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::cell::RefCell;
use worker::{wasm_bindgen, Env};

#[derive(Deserialize)]
struct Input {
    secret: Option<String>,
    body: Value,
    #[serde(default)]
    deliver: bool,
    #[serde(default)]
    fail_send: bool,
    #[serde(default)]
    timeout_send: bool,
    #[serde(default)]
    rate: bool,
    #[serde(default)]
    http: bool,
    #[serde(default)]
    export_http: bool,
    export_token: Option<String>,
    cursor: Option<String>,
    created_at: String,
}

struct TestClock(CreatedAt);
impl Clock for TestClock {
    fn now(&self) -> Result<CreatedAt> {
        Ok(self.0)
    }
}

struct TestTransport {
    fail: bool,
    timeout: bool,
    messages: RefCell<Vec<Value>>,
}
impl Transport for TestTransport {
    async fn send(&self, reply: &telegram::render::Reply) -> Result<()> {
        if self.fail {
            return Err(Error::Storage);
        }
        if self.timeout {
            return Err(Error::Timeout);
        }
        self.messages
            .borrow_mut()
            .push(serde_json::to_value(reply).map_err(|_| Error::Storage)?);
        Ok(())
    }
}

#[wasm_bindgen::prelude::wasm_bindgen(
    js_sys = worker::js_sys,
    wasm_bindgen_futures = worker::wasm_bindgen_futures
)]
pub async fn telegram_test_request(input: String, environment: Env) -> String {
    match execute(&input, environment).await {
        Ok(value) => json!({"ok": value}).to_string(),
        Err(error) => json!({"error": format!("{error:?}")}).to_string(),
    }
}

async fn execute(input: &str, environment: Env) -> Result<Value> {
    let input: Input = serde_json::from_str(input).map_err(|_| Error::Invalid)?;
    if input.export_http {
        return export_request(input, environment).await;
    }
    if input.http {
        return http_request(input, environment).await;
    }
    let config = Configuration::new(
        "fake_secret".into(),
        "SettleBot".into(),
        -100,
        vec![100, 101],
    )?;
    let clock = TestClock(CreatedAt::parse(&input.created_at)?);
    let repository = D1Repository::new(
        environment
            .d1(DATABASE_BINDING)
            .map_err(|_| Error::Storage)?,
    );
    let body = serde_json::to_vec(&input.body).map_err(|_| Error::Invalid)?;
    if input.rate {
        telegram::worker::admit_update(&config, &body, &repository, &clock).await?;
        return Ok(json!({"admitted": true}));
    }
    if input.deliver {
        let transport = TestTransport {
            fail: input.fail_send,
            timeout: input.timeout_send,
            messages: RefCell::new(Vec::new()),
        };
        let response = telegram::deliver(
            &config,
            input.secret.as_deref(),
            &body,
            &repository,
            &clock,
            &transport,
        )
        .await?;
        return Ok(json!({"response": response, "messages": transport.messages.into_inner()}));
    }
    let reply =
        telegram::handle(&config, input.secret.as_deref(), &body, &repository, &clock).await?;
    serde_json::to_value(reply).map_err(|_| Error::Storage)
}

async fn export_request(input: Input, environment: Env) -> Result<Value> {
    let headers = worker::Headers::new();
    if let Some(token) = input.export_token {
        headers
            .set("authorization", &format!("Bearer {token}"))
            .map_err(|_| Error::Invalid)?;
    }
    let mut init = worker::RequestInit::new();
    init.with_method(worker::Method::Get).with_headers(headers);
    let suffix = input
        .cursor
        .map_or(String::new(), |cursor| format!("?cursor={cursor}"));
    let request = worker::Request::new_with_init(
        &format!("https://example.test/admin/export{suffix}"),
        &init,
    )
    .map_err(|_| Error::Invalid)?
    .try_into()
    .map_err(|_| Error::Invalid)?;
    let response = crate::export::response(request, environment)
        .await
        .map_err(|_| Error::Storage)?;
    let cache_control = response
        .headers()
        .get(http::header::CACHE_CONTROL)
        .and_then(|value| value.to_str().ok());
    Ok(
        json!({"status": response.status().as_u16(), "body": response.body(),
        "cache_control": cache_control}),
    )
}

async fn http_request(input: Input, environment: Env) -> Result<Value> {
    let headers = worker::Headers::new();
    if let Some(secret) = input.secret {
        headers
            .set("x-telegram-bot-api-secret-token", &secret)
            .map_err(|_| Error::Invalid)?;
    }
    let mut init = worker::RequestInit::new();
    init.with_method(worker::Method::Post)
        .with_headers(headers)
        .with_body(Some(input.body.to_string().into()));
    let request = worker::Request::new_with_init("https://example.test/telegram/webhook", &init)
        .map_err(|_| Error::Invalid)?
        .try_into()
        .map_err(|_| Error::Invalid)?;
    let response = telegram::worker::webhook(request, environment)
        .await
        .map_err(|_| Error::Storage)?;
    Ok(json!({"status": response.status().as_u16(), "body": response.body()}))
}
