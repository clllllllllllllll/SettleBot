//! Feature-gated RPC bridge for the local workerd harness, absent from normal builds.

use serde::Deserialize;
use serde_json::{json, Value};
use worker::{wasm_bindgen, Env};

use crate::domain::{
    AmountEntry, Cents, CreatedAt, DisplayName, DraftSession, Game, GameId, Guest, GuestId,
    OccurredOn, Participant, Player, PlayerId, RosterStatus, SessionId,
};
use crate::persistence::{
    d1::{D1Repository, DATABASE_BINDING},
    ConfirmCommand, DraftState, Ledger, OwnerId, PersistenceError, PersistenceResult,
    PersistentDraft, Revision, UpdateKey,
};

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum Command {
    AddPlayer {
        id: String,
        name: String,
    },
    AddGame {
        id: String,
        name: String,
    },
    Catalog,
    SaveDraft {
        draft: DraftInput,
        expected_revision: Option<u32>,
    },
    Resume {
        id: String,
        owner: i64,
    },
    Confirm(ConfirmationInput),
    Session {
        id: String,
    },
}

#[wasm_bindgen::prelude::wasm_bindgen(
    js_sys = worker::js_sys,
    wasm_bindgen_futures = worker::wasm_bindgen_futures
)]
pub async fn persistence_test_command(input: String, environment: Env) -> String {
    let result = execute(&input, environment).await;
    match result {
        Ok(value) => json!({"ok": value}).to_string(),
        Err(error) => json!({"error": format!("{error:?}")}).to_string(),
    }
}

async fn execute(input: &str, environment: Env) -> PersistenceResult<Value> {
    let command =
        serde_json::from_str(input).map_err(|_| PersistenceError::Corrupt("test command"))?;
    let database = environment
        .d1(DATABASE_BINDING)
        .map_err(|_| PersistenceError::Storage)?;
    let repository = D1Repository::new(database);
    dispatch(command, &Ledger::new(&repository)).await
}

async fn dispatch(command: Command, ledger: &Ledger<'_, D1Repository>) -> PersistenceResult<Value> {
    match command {
        Command::AddPlayer { id, name } => add_player(ledger, &id, &name).await,
        Command::AddGame { id, name } => add_game(ledger, &id, &name).await,
        Command::Catalog => catalog(ledger).await,
        Command::SaveDraft {
            draft,
            expected_revision,
        } => save_draft(ledger, draft, expected_revision).await,
        Command::Resume { id, owner } => resume(ledger, &id, owner).await,
        Command::Confirm(input) => confirm(ledger, input).await,
        Command::Session { id } => session(ledger, &id).await,
    }
}

async fn resume(
    ledger: &Ledger<'_, D1Repository>,
    id: &str,
    owner: i64,
) -> PersistenceResult<Value> {
    let draft = ledger
        .resume(&SessionId::new(id)?, OwnerId::new(owner)?)
        .await?;
    Ok(draft_json(&draft))
}

async fn add_player(
    ledger: &Ledger<'_, D1Repository>,
    id: &str,
    name: &str,
) -> PersistenceResult<Value> {
    ledger
        .add_player(&Player::new(
            PlayerId::new(id)?,
            DisplayName::new(name)?,
            RosterStatus::Active,
        ))
        .await?;
    Ok(Value::Null)
}

async fn add_game(
    ledger: &Ledger<'_, D1Repository>,
    id: &str,
    name: &str,
) -> PersistenceResult<Value> {
    ledger
        .add_game(&Game::new(GameId::new(id)?, DisplayName::new(name)?))
        .await?;
    Ok(Value::Null)
}

async fn catalog(ledger: &Ledger<'_, D1Repository>) -> PersistenceResult<Value> {
    let players: Vec<_> = ledger
        .players()
        .await?
        .into_iter()
        .map(|player| json!({"id": player.id().as_str(), "name": player.name().as_str()}))
        .collect();
    let games: Vec<_> = ledger
        .games()
        .await?
        .into_iter()
        .map(|game| json!({"id": game.id().as_str(), "name": game.name().as_str()}))
        .collect();
    Ok(json!({"players": players, "games": games}))
}

#[derive(Deserialize)]
struct DraftInput {
    id: String,
    owner: i64,
    game: String,
    occurred_on: String,
    revision: u32,
    participants: Vec<ParticipantInput>,
}

#[derive(Deserialize)]
struct ParticipantInput {
    kind: String,
    id: String,
    name: Option<String>,
    guest_session: Option<String>,
    amount: Option<String>,
}

impl ParticipantInput {
    fn participant(&self, session: &SessionId) -> PersistenceResult<Participant> {
        if self.kind == "roster" {
            return Ok(Participant::Roster(PlayerId::new(&self.id)?));
        }
        let session = self.guest_scope(session)?;
        let name = self
            .name
            .as_deref()
            .ok_or(PersistenceError::Corrupt("test guest name"))?;
        Ok(Participant::Guest(Guest::new(
            GuestId::new(session, &self.id)?,
            DisplayName::new(name)?,
        )))
    }

    fn guest_scope(&self, session: &SessionId) -> PersistenceResult<SessionId> {
        match self.guest_session.as_deref() {
            Some(id) => Ok(SessionId::new(id)?),
            None => Ok(session.clone()),
        }
    }
}

impl DraftInput {
    fn decode(self) -> PersistenceResult<PersistentDraft> {
        let id = SessionId::new(&self.id)?;
        let (participants, entries) = decode_participant_inputs(&id, &self.participants)?;
        let draft = DraftSession::new(
            id,
            GameId::new(&self.game)?,
            OccurredOn::parse(&self.occurred_on)?,
            participants,
        )?;
        PersistentDraft::new(
            draft,
            OwnerId::new(self.owner)?,
            entries,
            Revision::new(self.revision),
            DraftState::Open,
        )
    }
}

fn decode_participant_inputs(
    id: &SessionId,
    inputs: &[ParticipantInput],
) -> PersistenceResult<(Vec<Participant>, Vec<AmountEntry>)> {
    let mut participants = Vec::new();
    let mut entries = Vec::new();
    for input in inputs {
        let participant = input.participant(id)?;
        if let Some(amount) = input.amount.as_deref() {
            let cents = amount
                .parse()
                .map_err(|_| PersistenceError::Corrupt("test cents"))?;
            entries.push(AmountEntry::new(participant.id(), Cents::new(cents)));
        }
        participants.push(participant);
    }
    Ok((participants, entries))
}

async fn save_draft(
    ledger: &Ledger<'_, D1Repository>,
    input: DraftInput,
    expected: Option<u32>,
) -> PersistenceResult<Value> {
    let draft = input.decode()?;
    match expected {
        Some(revision) => {
            ledger
                .replace_draft(&draft, Revision::new(revision))
                .await?
        }
        None => ledger.create_draft(&draft).await?,
    }
    Ok(draft_json(&draft))
}

fn draft_json(draft: &PersistentDraft) -> Value {
    json!({
        "id": draft.session().id().as_str(), "owner": draft.owner().value().to_string(),
        "game": draft.session().game_id().as_str(), "occurred_on": draft.session().occurred_on().to_string(),
        "revision": draft.revision().value(), "state": format!("{:?}", draft.state()),
        "participants": participant_json(draft.session().participants(), draft.entries()),
    })
}

fn participant_json(participants: &[Participant], entries: &[AmountEntry]) -> Vec<Value> {
    participants.iter().map(|participant| {
        let amount = entries.iter().find(|entry| entry.participant_id() == &participant.id()).map(|entry| entry.amount().value().to_string());
        match participant {
            Participant::Roster(id) => json!({"kind": "roster", "id": id.as_str(), "amount": amount}),
            Participant::Guest(guest) => json!({"kind": "guest", "id": guest.id().as_str(), "name": guest.name().as_str(), "session": guest.id().session_id().as_str(), "amount": amount}),
        }
    }).collect()
}

#[derive(Deserialize)]
struct ConfirmationInput {
    id: String,
    owner: i64,
    revision: u32,
    update_key: String,
    created_at: String,
}

async fn confirm(
    ledger: &Ledger<'_, D1Repository>,
    input: ConfirmationInput,
) -> PersistenceResult<Value> {
    let command = ConfirmCommand {
        update_key: UpdateKey::new(&input.update_key)?,
        draft_id: SessionId::new(&input.id)?,
        owner: OwnerId::new(input.owner)?,
        revision: Revision::new(input.revision),
        created_at: CreatedAt::parse(&input.created_at)?,
    };
    let result = ledger.confirm(&command).await?;
    Ok(
        json!({"id": result.session_id.as_str(), "owner": result.owner.value().to_string(), "created_at": result.created_at.to_string()}),
    )
}

async fn session(ledger: &Ledger<'_, D1Repository>, id: &str) -> PersistenceResult<Value> {
    let Some(session) = ledger.session(&SessionId::new(id)?).await? else {
        return Ok(Value::Null);
    };
    Ok(
        json!({"id": session.id().as_str(), "game": session.game_id().as_str(),
        "occurred_on": session.occurred_on().to_string(), "created_at": session.created_at().to_string(),
        "participants": participant_json(session.participants(), session.entries())}),
    )
}
