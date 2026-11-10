use super::{
    auth::decode,
    commands::{amount, parse, Command},
    flow::Flow,
    *,
};
use crate::domain::{Cents, CreatedAt, DisplayName, Game, GameId, Player};
use crate::persistence::{GameRepository, OwnerId, PersistenceResult, RosterRepository, UpdateKey};
use serde_json::{json, Value};
use std::cell::Cell;

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
        (
            "/correct@SettleBot t1 0 +1.00 -1.00 | Fix entry",
            "correct:t1:2:Fix entry",
        ),
        ("/void@SettleBot t1 0 | Duplicate", "void:t1:Duplicate"),
        ("/leaderboard@SettleBot 2", "leaderboard:2"),
        ("/gameboard@SettleBot poker 3", "gameboard:poker:3"),
        ("/stats@SettleBot player_one", "stats:player_one:1"),
        ("/history@SettleBot", "history:1"),
        ("/details@SettleBot t1", "details:t1"),
        ("/details@SettleBot t1 2", "details:t1:2"),
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
        Command::Correct {
            session,
            amounts,
            reason,
            ..
        } => format!("correct:{}:{}:{reason}", session.as_str(), amounts.len()),
        Command::Void {
            session, reason, ..
        } => {
            format!("void:{}:{reason}", session.as_str())
        }
        Command::Report(super::commands::ReportCommand::Leaderboard { page }) => {
            format!("leaderboard:{page}")
        }
        Command::Report(super::commands::ReportCommand::GameLeaderboard { game, page }) => {
            format!("gameboard:{}:{page}", game.as_str())
        }
        Command::Report(super::commands::ReportCommand::Player { player, page }) => {
            format!("stats:{}:{page}", player.as_str())
        }
        Command::Report(super::commands::ReportCommand::History { page }) => {
            format!("history:{page}")
        }
        Command::Report(super::commands::ReportCommand::Session {
            session,
            after_revision,
        }) => {
            if *after_revision == 0 {
                return format!("details:{}", session.as_str());
            }
            format!("details:{}:{after_revision}", session.as_str())
        }
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
        (Error::AuditStale, 200),
        (Error::Limit, 200),
        (Error::RateLimited, 200),
        (Error::Timeout, 503),
        (Error::TelegramNetwork, 503),
        (Error::TelegramAmbiguous, 503),
        (
            Error::TelegramRateLimited {
                retry_after: Some(4),
            },
            503,
        ),
        (Error::TelegramServer, 503),
        (Error::TelegramAccess, 503),
        (Error::TelegramRejected, 200),
        (Error::Storage, 503),
        (Error::Configuration, 503),
    ] {
        let response = worker::response(Err(error)).unwrap();
        assert_eq!(response.status().as_u16(), status);
        assert_eq!(response.headers()["content-type"], "application/json");
    }
}

#[test]
fn update_and_pagination_sizes_are_explicitly_bounded() {
    assert_eq!(PageSize::new(0), Err(Error::Limit));
    assert_eq!(PageSize::new(MAX_PAGE_SIZE + 1), Err(Error::Limit));
    assert_eq!(PageSize::new(MAX_PAGE_SIZE).unwrap().value(), MAX_PAGE_SIZE);
    assert_eq!(
        policy::catalog_capacity(policy::MAX_CATALOG_CHOICES - 1),
        Ok(())
    );
    assert_eq!(
        policy::catalog_capacity(policy::MAX_CATALOG_CHOICES),
        Err(Error::Limit)
    );

    let mut text = message();
    text["message"]["text"] = json!("x".repeat(policy::MAX_TEXT_BYTES + 1));
    assert!(matches!(
        decode(&config(), text.to_string().as_bytes()),
        Err(Error::Limit)
    ));

    let mut callback = callback_update("x");
    callback["callback_query"]["data"] = json!("x".repeat(policy::MAX_CALLBACK_BYTES + 1));
    assert!(matches!(
        decode(&config(), callback.to_string().as_bytes()),
        Err(Error::Limit)
    ));
}

#[test]
fn catalog_snapshots_have_an_explicit_upper_bound() {
    let games = (0..=policy::MAX_CATALOG_CHOICES)
        .map(|index| {
            Game::new(
                GameId::new(&format!("g{index}")).unwrap(),
                DisplayName::new("Game").unwrap(),
            )
        })
        .collect();
    assert!(matches!(
        Flow::new("t1".into(), OwnerId::new(100).unwrap(), -100, games, vec![]),
        Err(Error::Limit)
    ));
}

#[test]
fn guided_selection_enforces_the_domain_participant_limit() {
    let selected = (0..crate::domain::MAX_SESSION_PARTICIPANTS)
        .map(|index| flow::Selection {
            choice: flow::Choice {
                id: format!("guest_{index}"),
                name: "Sample Guest".into(),
            },
            guest: true,
            cents: None,
        })
        .collect();
    let mut flow = participant_flow(selected);
    assert_eq!(
        flow.advance(commands::Action::Guest("Another Guest".into())),
        Err(Error::Limit)
    );
}

fn participant_flow(selected: Vec<flow::Selection>) -> Flow {
    Flow {
        id: "t1".into(),
        owner: 100,
        chat: -100,
        revision: 0,
        stage: flow::Stage::Participants,
        games: vec![],
        roster: vec![],
        game: Some(flow::Choice {
            id: "poker".into(),
            name: "Poker".into(),
        }),
        date: Some("2026-11-01".into()),
        selected,
    }
}

#[test]
fn only_idempotent_rate_admission_is_retried_and_recovers() {
    let request = rate_request();
    let transient = FailingLimiter::new(1);
    assert_eq!(run(admit_with_retry(&transient, &request)), Ok(()));
    assert_eq!(transient.calls.get(), policy::SAFE_RETRY_ATTEMPTS);

    let unavailable = FailingLimiter::new(policy::SAFE_RETRY_ATTEMPTS);
    assert_eq!(
        run(admit_with_retry(&unavailable, &request)),
        Err(Error::Storage)
    );
    assert_eq!(unavailable.calls.get(), policy::SAFE_RETRY_ATTEMPTS);
}

#[test]
fn deadlines_report_completion_and_timeout_without_sleeping() {
    let ready = std::future::ready(7);
    let never = std::future::pending::<()>();
    assert_eq!(run(resilience::with_deadline(ready, never)), Ok(7));
    let never = std::future::pending::<u8>();
    assert_eq!(
        run(resilience::with_deadline(never, std::future::ready(()))),
        Err(())
    );
}

#[test]
fn structured_failure_events_never_contain_request_data() {
    let sensitive = ["fake_secret", "Private Player", "-1250", "telegram_123"];
    for error in [
        Error::Rejected,
        Error::Limit,
        Error::Timeout,
        Error::Storage,
    ] {
        let encoded = serde_json::to_string(&logging::event(&error)).unwrap();
        assert!(sensitive.iter().all(|value| !encoded.contains(value)));
        assert!(encoded.contains("telegram_webhook_failed"));
    }
}

fn callback_update(data: &str) -> Value {
    json!({"update_id": 2, "callback_query": {"id": "fake", "from": {"id": 100,
        "is_bot": false}, "message": message()["message"], "data": data}})
}

struct CountingTransport(Cell<usize>);
impl Transport for CountingTransport {
    async fn send(&self, _: &render::Reply) -> Result<()> {
        self.0.set(self.0.get() + 1);
        Ok(())
    }
}

#[test]
fn unrelated_messages_bypass_admission_storage_and_delivery_after_authorization() {
    let limiter = FailingLimiter::new(0);
    let transport = CountingTransport(Cell::new(0));
    for text in ["ordinary group text", "hello@OtherBot", "  "] {
        let body = message_with_text(text);
        assert_eq!(
            run(transport::receive(
                &config(),
                Some("fake_secret"),
                body.to_string().as_bytes(),
                &NoAccess,
                &FixedClock,
                &transport,
                &limiter
            )),
            Ok(json!({"ok": true}))
        );
    }
    assert_eq!(limiter.calls.get(), 0);
    assert_eq!(transport.0.get(), 0);
}

#[test]
fn malformed_commands_and_callbacks_still_require_admission() {
    let limiter = FailingLimiter::new(0);
    let transport = CountingTransport(Cell::new(0));
    for body in [
        message_with_text("/step"),
        message_with_text("/guest"),
        message_with_text("/unknown"),
        callback_update("malformed"),
    ] {
        assert!(run(transport::receive(
            &config(),
            Some("fake_secret"),
            body.to_string().as_bytes(),
            &NoAccess,
            &FixedClock,
            &transport,
            &limiter
        ))
        .is_ok());
    }
    assert_eq!(limiter.calls.get(), 4);
    assert_eq!(transport.0.get(), 4);
}

fn message_with_text(text: &str) -> Value {
    let mut body = message();
    body["message"]["text"] = json!(text);
    body
}

fn rate_request() -> RateLimitRequest {
    RateLimitRequest {
        key: UpdateKey::new("telegram_1").unwrap(),
        owner: OwnerId::new(100).unwrap(),
        now: CreatedAt::parse("2026-11-01T16:00:00Z").unwrap(),
    }
}

#[test]
fn bot_api_responses_are_classified_conservatively() {
    for (status, body, expected) in [
        (200, json!({"ok":true,"result":{"message_id":1}}), Ok(())),
        (
            400,
            json!({"ok":false,"error_code":400,"description":"Bad Request: message is too long"}),
            Err(Error::TelegramRejected),
        ),
        (
            400,
            json!({"ok":false,"error_code":400,"description":"new error"}),
            Err(Error::TelegramAmbiguous),
        ),
        (
            401,
            json!({"ok":false,"error_code":401}),
            Err(Error::TelegramAccess),
        ),
        (
            429,
            json!({"ok":false,"error_code":429,"parameters":{"retry_after":4}}),
            Err(Error::TelegramRateLimited {
                retry_after: Some(4),
            }),
        ),
        (
            429,
            json!({"ok":false,"error_code":429}),
            Err(Error::TelegramRateLimited { retry_after: None }),
        ),
        (
            400,
            json!({"ok":false,"error_code":400,"parameters":{"migrate_to_chat_id":-200}}),
            Err(Error::TelegramAccess),
        ),
        (
            400,
            json!({"ok":false,"error_code":403}),
            Err(Error::TelegramAmbiguous),
        ),
        (200, json!({"ok":true}), Err(Error::TelegramAmbiguous)),
    ] {
        assert_eq!(
            bot_api::classify(status, body.to_string().as_bytes()),
            expected
        );
    }
    assert_eq!(
        bot_api::classify(503, b"unreadable"),
        Err(Error::TelegramServer)
    );
    assert_eq!(
        bot_api::classify(400, b"unreadable"),
        Err(Error::TelegramAmbiguous)
    );
}

struct FailingLimiter {
    failures: Cell<usize>,
    calls: Cell<usize>,
}

impl FailingLimiter {
    fn new(failures: usize) -> Self {
        Self {
            failures: Cell::new(failures),
            calls: Cell::new(0),
        }
    }
}

impl RateLimiter for FailingLimiter {
    async fn admit(&self, _: &RateLimitRequest) -> Result<bool> {
        self.calls.set(self.calls.get() + 1);
        if self.failures.get() == 0 {
            return Ok(true);
        }
        self.failures.set(self.failures.get() - 1);
        Err(Error::Storage)
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

#[test]
fn audit_commands_require_an_explicit_valid_revision() {
    for text in [
        "/correct t1 +1.00 -1.00 | reason",
        "/void t1 | reason",
        "/void t1 -1 | reason",
        "/void t1 +1 | reason",
        "/void t1 9223372036854775808 | reason",
    ] {
        let update = decode(&config(), message_with_text(text).to_string().as_bytes()).unwrap();
        assert!(
            matches!(parse(&update, "SettleBot"), Err(Error::Invalid)),
            "{text}"
        );
    }
    let update = decode(
        &config(),
        message_with_text("/correct t1 42 +1.00 -1.00 | reason")
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    assert!(
        matches!(parse(&update, "SettleBot"), Ok(Command::Correct { revision, .. }) if revision.value() == 42)
    );
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
impl crate::persistence::ReportingRepository for NoAccess {
    async fn leaderboard(
        &self,
        _: crate::persistence::ReportPageRequest,
    ) -> PersistenceResult<crate::persistence::ReportPage<crate::persistence::LeaderboardEntry>>
    {
        panic!("unauthorized read")
    }
    async fn game_leaderboard(
        &self,
        _: &crate::domain::GameId,
        _: crate::persistence::ReportPageRequest,
    ) -> PersistenceResult<Option<crate::persistence::GameLeaderboard>> {
        panic!("unauthorized read")
    }
    async fn player_report(
        &self,
        _: &crate::domain::PlayerId,
        _: crate::persistence::ReportPageRequest,
    ) -> PersistenceResult<Option<crate::persistence::PlayerReport>> {
        panic!("unauthorized read")
    }
    async fn session_history(
        &self,
        _: crate::persistence::ReportPageRequest,
    ) -> PersistenceResult<crate::persistence::ReportPage<crate::persistence::SessionSummary>> {
        panic!("unauthorized read")
    }
    async fn session_detail(
        &self,
        _: &crate::domain::SessionId,
        _: crate::persistence::AuditPageRequest,
    ) -> PersistenceResult<Option<crate::persistence::SessionDetail>> {
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
