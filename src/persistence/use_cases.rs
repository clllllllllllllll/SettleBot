use crate::domain::{CreatedAt, Game, Player, SessionId};

use super::{
    Confirmation, ConfirmationRepository, DraftRepository, DraftState, GameRepository, OwnerId,
    PersistenceError, PersistenceResult, PersistentDraft, Revision, RosterRepository,
    SessionRepository, UpdateKey,
};

pub struct Ledger<'a, R> {
    repository: &'a R,
}

impl<'a, R> Ledger<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }
}

impl<R: RosterRepository> Ledger<'_, R> {
    pub async fn add_player(&self, player: &Player) -> PersistenceResult<()> {
        self.repository.add_player(player).await
    }

    pub async fn players(&self) -> PersistenceResult<Vec<Player>> {
        self.repository.players().await
    }
}

impl<R: GameRepository> Ledger<'_, R> {
    pub async fn add_game(&self, game: &Game) -> PersistenceResult<()> {
        self.repository.add_game(game).await
    }

    pub async fn games(&self) -> PersistenceResult<Vec<Game>> {
        self.repository.games().await
    }
}

impl<R: DraftRepository> Ledger<'_, R> {
    pub async fn create_draft(&self, draft: &PersistentDraft) -> PersistenceResult<()> {
        draft.check_edit(draft.owner(), Revision::default())?;
        self.repository.create_draft(draft).await
    }

    pub async fn replace_draft(
        &self,
        draft: &PersistentDraft,
        expected: Revision,
    ) -> PersistenceResult<()> {
        if draft.state() != DraftState::Open || draft.revision() != expected.next()? {
            return Err(PersistenceError::RevisionConflict);
        }
        self.repository.replace_draft(draft, expected).await
    }

    pub async fn resume(
        &self,
        id: &SessionId,
        owner: OwnerId,
    ) -> PersistenceResult<PersistentDraft> {
        let draft = self
            .repository
            .draft(id)
            .await?
            .ok_or(PersistenceError::NotFound)?;
        if draft.owner() != owner {
            return Err(PersistenceError::OwnerMismatch);
        }
        Ok(draft)
    }
}

impl<R: SessionRepository> Ledger<'_, R> {
    pub async fn session(
        &self,
        id: &SessionId,
    ) -> PersistenceResult<Option<crate::domain::ConfirmedSession>> {
        self.repository.session(id).await
    }
}

pub struct ConfirmCommand {
    pub update_key: UpdateKey,
    pub draft_id: SessionId,
    pub owner: OwnerId,
    pub revision: Revision,
    pub created_at: CreatedAt,
}

impl<R: ConfirmationRepository> Ledger<'_, R> {
    pub async fn confirm(&self, command: &ConfirmCommand) -> PersistenceResult<Confirmation> {
        if let Some(result) = self
            .repository
            .processed_update(&command.update_key)
            .await?
        {
            return replay(result, command);
        }
        match self.attempt_confirmation(command).await {
            Ok(result) => Ok(result),
            Err(error) => self.resolve_conflict(command, error).await,
        }
    }

    async fn attempt_confirmation(
        &self,
        command: &ConfirmCommand,
    ) -> PersistenceResult<Confirmation> {
        let draft = self.resume(&command.draft_id, command.owner).await?;
        draft.check_edit(command.owner, command.revision)?;
        command.revision.next()?;
        let session = draft.confirm(command.created_at)?;
        self.repository
            .commit_confirmation(
                &command.update_key,
                command.owner,
                command.revision,
                &session,
            )
            .await
    }

    async fn resolve_conflict(
        &self,
        command: &ConfirmCommand,
        error: PersistenceError,
    ) -> PersistenceResult<Confirmation> {
        if let Some(result) = self
            .repository
            .processed_update(&command.update_key)
            .await?
        {
            return replay(result, command);
        }
        let draft = self.resume(&command.draft_id, command.owner).await?;
        draft.check_edit(command.owner, command.revision)?;
        Err(error)
    }
}

fn replay(result: Confirmation, command: &ConfirmCommand) -> PersistenceResult<Confirmation> {
    if result.owner != command.owner || result.session_id != command.draft_id {
        return Err(PersistenceError::UpdateKeyConflict);
    }
    Ok(result)
}
