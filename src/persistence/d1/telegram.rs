use super::D1Repository;
use crate::domain::{ConfirmedSession, Game, Player};
use crate::persistence::{PersistentDraft, UpdateKey};
use crate::telegram::{flow::Flow, Change, Error, Receipt, Repository, Result, Transaction};
use worker::{D1PreparedStatement, D1Type::Text};

const FLOW: &str =
    "SELECT id, owner_id, chat_id, revision, state, document FROM telegram_flows WHERE id = ?";
const RECEIPT: &str = "SELECT fingerprint, response FROM telegram_updates WHERE update_key = ?";
const SAVE_RECEIPT: &str =
    "INSERT INTO telegram_updates (update_key, fingerprint, response) VALUES (?, ?, ?)";
// A failed compare-and-swap violates the revision constraint, aborting the batch.
const SAVE_FLOW: &str = "
    INSERT INTO telegram_flows (id, owner_id, chat_id, revision, state, document)
    VALUES (?1, ?2, ?3, CASE WHEN ?4 = '0' OR EXISTS (
        SELECT 1 FROM telegram_flows WHERE id = ?1 AND owner_id = ?2 AND chat_id = ?3
        AND revision = CAST(?4 AS INTEGER) - 1 AND state = 'open')
        THEN CAST(?4 AS INTEGER) ELSE -1 END, ?5, ?6)
    ON CONFLICT(id) DO UPDATE SET
        revision = CASE WHEN telegram_flows.owner_id = ?2 AND telegram_flows.chat_id = ?3
            AND telegram_flows.revision = CAST(?4 AS INTEGER) - 1 AND telegram_flows.state = 'open'
            THEN CAST(?4 AS INTEGER) ELSE -1 END,
        state = ?5, document = ?6";

#[derive(serde::Deserialize)]
struct FlowRow {
    id: String,
    owner_id: String,
    chat_id: String,
    revision: u32,
    state: String,
    document: String,
}

impl FlowRow {
    fn decode(self) -> Result<Flow> {
        let flow: Flow = serde_json::from_str(&self.document).map_err(|_| Error::Storage)?;
        if flow.id != self.id
            || flow.owner.to_string() != self.owner_id
            || flow.chat.to_string() != self.chat_id
            || flow.revision != self.revision
            || flow.storage_state() != self.state
        {
            return Err(Error::Storage);
        }
        flow.validate().map_err(|_| Error::Storage)?;
        Ok(flow)
    }
}

impl Repository for D1Repository {
    async fn flow(&self, id: &str) -> Result<Option<Flow>> {
        self.rows::<FlowRow>(FLOW, &[Text(id)])
            .await?
            .into_iter()
            .next()
            .map(FlowRow::decode)
            .transpose()
    }

    async fn receipt(&self, key: &UpdateKey) -> Result<Option<Receipt>> {
        Ok(self
            .rows(RECEIPT, &[Text(key.as_str())])
            .await?
            .into_iter()
            .next())
    }

    async fn apply(&self, transaction: &Transaction) -> Result<()> {
        let mut statements = self.telegram_statements(transaction)?;
        let reply = serde_json::to_string(&transaction.reply).map_err(|_| Error::Storage)?;
        statements.push(self.statement(
            SAVE_RECEIPT,
            &[
                Text(transaction.key.as_str()),
                Text(&transaction.fingerprint),
                Text(&reply),
            ],
        )?);
        self.batch(statements).await?;
        Ok(())
    }
}

impl D1Repository {
    fn save_flow(&self, flow: &Flow) -> Result<D1PreparedStatement> {
        let document = serde_json::to_string(flow).map_err(|_| Error::Storage)?;
        Ok(self.statement(
            SAVE_FLOW,
            &[
                Text(&flow.id),
                Text(&flow.owner.to_string()),
                Text(&flow.chat.to_string()),
                Text(&flow.revision.to_string()),
                Text(flow.storage_state()),
                Text(&document),
            ],
        )?)
    }

    fn telegram_statements(&self, transaction: &Transaction) -> Result<Vec<D1PreparedStatement>> {
        match &transaction.change {
            Change::Flow(flow) => Ok(vec![self.save_flow(flow)?]),
            Change::Player(player) => Ok(vec![self.telegram_player(player)?]),
            Change::Game(game) => Ok(vec![self.telegram_game(game)?]),
            Change::Confirm {
                flow,
                draft,
                session,
            } => self.telegram_confirmation(&transaction.key, flow, draft, session),
        }
    }

    fn telegram_player(&self, player: &Player) -> Result<D1PreparedStatement> {
        Ok(self.statement(
            "INSERT INTO players (id, display_name, status) VALUES (?, ?, 'active')",
            &[Text(player.id().as_str()), Text(player.name().as_str())],
        )?)
    }

    fn telegram_game(&self, game: &Game) -> Result<D1PreparedStatement> {
        Ok(self.statement(
            "INSERT INTO games (id, display_name) VALUES (?, ?)",
            &[Text(game.id().as_str()), Text(game.name().as_str())],
        )?)
    }

    fn telegram_confirmation(
        &self,
        key: &UpdateKey,
        flow: &Flow,
        draft: &PersistentDraft,
        session: &ConfirmedSession,
    ) -> Result<Vec<D1PreparedStatement>> {
        let mut statements = vec![self.save_flow(flow)?];
        statements.extend(self.create_statements(draft)?);
        statements.extend(self.confirmation_statements(
            key,
            draft.owner(),
            draft.revision(),
            session,
        )?);
        Ok(statements)
    }
}
