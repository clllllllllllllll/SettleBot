use super::{flow::Flow, render::Reply, Result};
use crate::domain::{ConfirmedSession, Game, Player};
use crate::persistence::{GameRepository, PersistentDraft, RosterRepository, UpdateKey};

pub(crate) enum Change {
    Flow(Flow),
    Player(Player),
    Game(Game),
    Confirm {
        flow: Flow,
        draft: Box<PersistentDraft>,
        session: Box<ConfirmedSession>,
    },
}

pub(crate) struct Transaction {
    pub key: UpdateKey,
    pub fingerprint: String,
    pub change: Change,
    pub reply: Reply,
}

#[derive(serde::Deserialize)]
pub(crate) struct Receipt {
    pub fingerprint: String,
    pub response: String,
}

#[allow(async_fn_in_trait)]
pub(crate) trait Repository: RosterRepository + GameRepository {
    async fn flow(&self, id: &str) -> Result<Option<Flow>>;
    async fn receipt(&self, key: &UpdateKey) -> Result<Option<Receipt>>;
    /// Atomically commit the mutation, optimistic workflow revision and durable reply.
    async fn apply(&self, transaction: &Transaction) -> Result<()>;
}
