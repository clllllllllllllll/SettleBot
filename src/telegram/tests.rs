use super::{
    auth::decode,
    commands::{amount, parse, Command},
    flow::Flow,
    *,
};
use crate::domain::{Cents, CreatedAt, Game, Player};
use crate::persistence::{GameRepository, PersistenceResult, RosterRepository, UpdateKey};
use serde_json::{json, Value};

fn config() -> Configuration {
    Configuration::new(
        "fake_secret".into(),
        "SettleBot".into(),
        -100,
        vec![100, 101],
    )
    .unwrap()
}

fn message() -> Value {
    json!({"update_id": 1, "message": {"date": 1, "chat": {"id": -100, "type": "supergroup"},
        "from": {"id": 100, "is_bot": false}, "text": "/session"}})
}

#[test]
fn secret_is_required_and_checked_before_json_or_repository_access() {
    for secret in [
        None,
        Some(""),
        Some("fake_secreu"),
        Some("fake_secret_extra"),
    ] {
        assert_eq!(
            run(handle(
                &config(),
                secret,
                b"not json",
                &NoAccess,
                &FixedClock
            )),
            Err(Error::Unauthorized)
        );
    }
}

#[test]
fn configuration_fails_closed() {
    for (secret, chat, admins) in [
        ("", -100, vec![100]),
        ("bad token", -100, vec![100]),
        ("fake", 100, vec![100]),
        ("fake", -100, vec![]),
        ("fake", -100, vec![0]),
    ] {
        assert!(Configuration::new(secret.into(), "SettleBot".into(), chat, admins).is_err());
    }
    for username in ["", "@SettleBot", "Settle-Bot", "Settle Bot"] {
        assert!(Configuration::new("fake".into(), username.into(), -100, vec![100]).is_err());
    }
}

#[test]
fn untrusted_chat_types_users_and_anonymous_senders_never_access_storage() {
    let cases = [
        ("/message/chat/type", json!("private")),
        ("/message/chat/type", json!("channel")),
        ("/message/chat/id", json!(-200)),
        ("/message/from/id", json!(999)),
        ("/message/from/is_bot", json!(true)),
        ("/message/from", Value::Null),
    ];
    for (pointer, value) in cases {
        let mut body = message();
        *body.pointer_mut(pointer).unwrap() = value;
        rejected(body);
    }
    let mut body = message();
    body["message"]["sender_chat"] = json!({"id": -100});
    rejected(body);
}

#[test]
fn usernames_names_and_telegram_admin_status_never_authorize() {
    let mut body = message();
    body["message"]["from"] = json!({"id": 999, "is_bot": false, "username": "owner",
        "first_name": "Administrator", "status": "creator", "is_admin": true});
    rejected(body);
}

#[test]
fn revoked_administrator_cannot_replay_previously_authorized_updates() {
    let config =
        Configuration::new("fake_secret".into(), "SettleBot".into(), -100, vec![101]).unwrap();
    assert_eq!(
        run(handle(
            &config,
            Some("fake_secret"),
            message().to_string().as_bytes(),
            &NoAccess,
            &FixedClock
        )),
        Err(Error::Rejected)
    );
}

#[test]
fn text_commands_accept_the_telegram_bot_username_suffix() {
    for (text, expected) in [
        ("/session@SettleBot", "session"),
        ("/player@SettleBot Sample Player", "player:Sample Player"),
        ("/game@SettleBot Sample Game", "game:Sample Game"),
        ("/resume@SettleBot t1", "resume:t1"),
        ("/step@SettleBot t1 2 +1.00", "step:t1:2:+1.00"),
        (
            "/guest@SettleBot t1 2 Sample Guest",
            "guest:t1:2:Sample Guest",
        ),
    ] {
        let mut body = message();
        body["message"]["text"] = json!(text);
        let update = decode(&config(), body.to_string().as_bytes()).unwrap();
        let parsed = parse(&update, config().bot_username()).unwrap();
        assert_eq!(command_description(&parsed), expected, "{text}");
    }

    let mut body = message();
    body["message"]["text"] = json!("/player@OtherBot Forbidden");
    let update = decode(&config(), body.to_string().as_bytes()).unwrap();
    assert!(matches!(
        parse(&update, config().bot_username()),
        Err(Error::Rejected)
    ));
    assert_eq!(
        run(handle(
            &config(),
            Some("fake_secret"),
            body.to_string().as_bytes(),
            &NoAccess,
            &FixedClock,
        )),
        Err(Error::Rejected)
    );
}

fn command_description(command: &Command) -> String {
    match command {
        Command::Start => "session".into(),
        Command::Player(name) => format!("player:{name}"),
        Command::Game(name) => format!("game:{name}"),
        Command::Resume(id) => format!("resume:{}", id.as_str()),
        Command::Step {
            id,
            revision,
            action: super::commands::Action::Guest(name),
            ..
        } => format!("guest:{}:{}:{name}", id.as_str(), revision.value()),
        Command::Step {
            id,
            revision,
            action: super::commands::Action::Text(value),
        } => format!("step:{}:{}:{value}", id.as_str(), revision.value()),
        Command::Step { .. } => "callback".into(),
    }
}

#[test]
fn group_and_supergroup_numeric_identity_are_supported() {
    for kind in ["group", "supergroup"] {
        let mut body = message();
        body["message"]["chat"]["type"] = json!(kind);
        assert!(decode(&config(), body.to_string().as_bytes()).is_ok());
    }
}

#[test]
fn business_account_messages_cannot_reuse_the_allowed_chat_identity() {
    let mut body = message();
    body["message"]["business_connection_id"] = json!("fake_business");
    rejected(body);
}

#[test]
fn http_errors_are_safe_and_retry_only_storage_or_configuration_failures() {
    for (error, status) in [
        (Error::Unauthorized, 403),
        (Error::Rejected, 200),
        (Error::Invalid, 200),
        (Error::Stale, 200),
        (Error::Storage, 503),
        (Error::Configuration, 503),
    ] {
        let response = worker::response(Err(error)).unwrap();
        assert_eq!(response.status().as_u16(), status);
        assert_eq!(response.headers()["content-type"], "application/json");
    }
}

#[test]
fn unsupported_and_unverifiable_updates_never_access_storage() {
    for body in [
        json!({"update_id":1,"channel_post":message()["message"]}),
        json!({"update_id":1,"edited_message":message()["message"]}),
        json!({"update_id":1,"callback_query":{"id":"fake","from":{"id":100,"is_bot":false},"data":"t1:0:confirm"}}),
    ] {
        rejected(body);
    }
}

#[test]
fn callbacks_authorize_the_clicking_user_and_require_accessible_group_message() {
    let mut body = json!({"update_id":2,"callback_query":{"id":"fake", "from":{"id":999,"is_bot":false},
        "message":message()["message"], "data":"t1:0:confirm"}});
    rejected(body.clone());
    body["callback_query"]["from"]["id"] = json!(100);
    assert!(decode(&config(), body.to_string().as_bytes()).is_ok());
    body["callback_query"]["message"]["date"] = json!(0);
    rejected(body);
}

#[test]
fn signed_sgd_parser_preserves_full_integer_boundaries_without_rounding() {
    for (input, cents) in [
        ("+0.00", 0),
        ("-12.50", -1250),
        ("+92233720368547758.07", i64::MAX),
        ("-92233720368547758.08", i64::MIN),
    ] {
        assert_eq!(amount(input), Ok(Cents::new(cents)));
    }
    for input in [
        "1.00",
        "+1",
        "+1.1",
        "+1.001",
        "+.00",
        "+1e2.00",
        "NaN",
        "- 1.00",
        "+92233720368547758.08",
        "-92233720368547758.09",
        "+9999999999999999999999999999999999999999.00",
    ] {
        assert_eq!(amount(input), Err(Error::Invalid), "{input}");
    }
}

fn rejected(body: Value) {
    assert!(run(handle(
        &config(),
        Some("fake_secret"),
        body.to_string().as_bytes(),
        &NoAccess,
        &FixedClock
    ))
    .is_err());
}

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> Result<CreatedAt> {
        Ok(CreatedAt::parse("2026-11-01T16:00:00Z").unwrap())
    }
}

// Any repository call in an unauthorized request fails the test, including reads.
struct NoAccess;
impl RosterRepository for NoAccess {
    async fn add_player(&self, _: &Player) -> PersistenceResult<()> {
        panic!("unauthorized write")
    }
    async fn players(&self) -> PersistenceResult<Vec<Player>> {
        panic!("unauthorized read")
    }
}
impl GameRepository for NoAccess {
    async fn add_game(&self, _: &Game) -> PersistenceResult<()> {
        panic!("unauthorized write")
    }
    async fn games(&self) -> PersistenceResult<Vec<Game>> {
        panic!("unauthorized read")
    }
}
impl Repository for NoAccess {
    async fn flow(&self, _: &str) -> Result<Option<Flow>> {
        panic!("unauthorized read")
    }
    async fn receipt(&self, _: &UpdateKey) -> Result<Option<Receipt>> {
        panic!("unauthorized read")
    }
    async fn apply(&self, _: &Transaction) -> Result<()> {
        panic!("unauthorized write")
    }
}

fn run<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    match future.as_mut().poll(&mut context) {
        std::task::Poll::Ready(value) => value,
        std::task::Poll::Pending => panic!("native test future unexpectedly pending"),
    }
}
