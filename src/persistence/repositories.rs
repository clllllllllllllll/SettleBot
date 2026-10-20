use crate::domain::{ConfirmedSession, Game, Player, SessionId};

use super::{Confirmation, OwnerId, PersistenceResult, PersistentDraft, Revision, UpdateKey};

// Workers futures need not be Send; no executor or async-trait dependency is required.
#[allow(async_fn_in_trait)]
pub trait RosterRepository {
    async fn add_player(&self, player: &Player) -> PersistenceResult<()>;
    async fn players(&self) -> PersistenceResult<Vec<Player>>;
}

#[allow(async_fn_in_trait)]
pub trait GameRepository {
    async fn add_game(&self, game: &Game) -> PersistenceResult<()>;
    async fn games(&self) -> PersistenceResult<Vec<Game>>;
}

#[allow(async_fn_in_trait)]
pub trait DraftRepository {
    async fn create_draft(&self, draft: &PersistentDraft) -> PersistenceResult<()>;
    async fn replace_draft(
        &self,
        draft: &PersistentDraft,
        expected: Revision,
    ) -> PersistenceResult<()>;
    async fn draft(&self, id: &SessionId) -> PersistenceResult<Option<PersistentDraft>>;
}

#[allow(async_fn_in_trait)]
pub trait SessionRepository {
    async fn session(&self, id: &SessionId) -> PersistenceResult<Option<ConfirmedSession>>;
}

#[allow(async_fn_in_trait)]
pub trait UpdateRepository {
    async fn processed_update(&self, key: &UpdateKey) -> PersistenceResult<Option<Confirmation>>;
}

#[allow(async_fn_in_trait)]
pub trait ConfirmationRepository: DraftRepository + UpdateRepository {
    /// Commit the session, entries, draft consumption and result in one transaction.
    async fn commit_confirmation(
        &self,
        key: &UpdateKey,
        owner: OwnerId,
        revision: Revision,
        session: &ConfirmedSession,
    ) -> PersistenceResult<Confirmation>;
}
