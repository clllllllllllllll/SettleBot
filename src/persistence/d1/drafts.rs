use worker::{
    D1PreparedStatement,
    D1Type::{Null, Text},
};

use crate::domain::{AmountEntry, Participant, SessionId};
use crate::persistence::{
    DraftRepository, PersistenceError, PersistenceResult, PersistentDraft, Revision,
};

use super::{
    records::{corrupt, participant_arguments, DraftRow},
    sql, D1Repository,
};

impl DraftRepository for D1Repository {
    async fn create_draft(&self, draft: &PersistentDraft) -> PersistenceResult<()> {
        draft.check_edit(draft.owner(), Revision::default())?;
        let mut statements = vec![self.create_header(draft)?];
        self.draft_participants(&mut statements, draft)?;
        match self.batch(statements).await {
            Ok(_) => Ok(()),
            Err(_) if self.draft(draft.session().id()).await?.is_some() => {
                Err(PersistenceError::IdentityConflict)
            }
            Err(error) => Err(error),
        }
    }

    async fn replace_draft(
        &self,
        draft: &PersistentDraft,
        expected: Revision,
    ) -> PersistenceResult<()> {
        self.check_current_edit(draft, expected).await?;
        draft.check_edit(draft.owner(), expected.next()?)?;
        if let Err(error) = self.batch(self.replace_statements(draft, expected)?).await {
            self.check_current_edit(draft, expected).await?;
            return Err(error);
        }
        Ok(())
    }

    async fn draft(&self, id: &SessionId) -> PersistenceResult<Option<PersistentDraft>> {
        self.snapshot::<DraftRow>(sql::DRAFT, sql::DRAFT_PARTICIPANTS, id)
            .await?
            .map(|(header, rows)| header.decode(rows).map_err(corrupt))
            .transpose()
    }
}

impl D1Repository {
    async fn check_current_edit(
        &self,
        draft: &PersistentDraft,
        expected: Revision,
    ) -> PersistenceResult<()> {
        self.draft(draft.session().id())
            .await?
            .ok_or(PersistenceError::NotFound)?
            .check_edit(draft.owner(), expected)
    }

    fn create_header(&self, draft: &PersistentDraft) -> PersistenceResult<D1PreparedStatement> {
        let owner = draft.owner().value().to_string();
        let date = draft.session().occurred_on().to_string();
        self.statement(
            sql::CREATE_DRAFT,
            &[
                Text(draft.session().id().as_str()),
                Text(&owner),
                Text(draft.session().game_id().as_str()),
                Text(&date),
            ],
        )
    }

    fn replace_statements(
        &self,
        draft: &PersistentDraft,
        expected: Revision,
    ) -> PersistenceResult<Vec<D1PreparedStatement>> {
        let mut statements = vec![self.replace_header(draft, expected)?];
        statements.push(self.statement(
            sql::CLEAR_PARTICIPANTS,
            &[Text(draft.session().id().as_str())],
        )?);
        self.draft_participants(&mut statements, draft)?;
        Ok(statements)
    }

    fn replace_header(
        &self,
        draft: &PersistentDraft,
        expected: Revision,
    ) -> PersistenceResult<D1PreparedStatement> {
        let owner = draft.owner().value().to_string();
        let date = draft.session().occurred_on().to_string();
        let expected = expected.value().to_string();
        let revision = draft.revision().value().to_string();
        self.statement(
            sql::REPLACE_DRAFT,
            &[
                Text(draft.session().id().as_str()),
                Text(&owner),
                Text(draft.session().game_id().as_str()),
                Text(&date),
                Text(&expected),
                Text(&revision),
            ],
        )
    }

    fn draft_participants(
        &self,
        statements: &mut Vec<D1PreparedStatement>,
        draft: &PersistentDraft,
    ) -> PersistenceResult<()> {
        for (position, participant) in draft.session().participants().iter().enumerate() {
            statements.push(self.participant_statement(
                sql::ADD_DRAFT_PARTICIPANT,
                draft.session().id(),
                position,
                participant,
                draft.entries(),
            )?);
        }
        Ok(())
    }

    pub(super) fn participant_statement(
        &self,
        sql: &str,
        id: &SessionId,
        position: usize,
        participant: &Participant,
        entries: &[AmountEntry],
    ) -> PersistenceResult<D1PreparedStatement> {
        let position = position.to_string();
        let amount = entries
            .iter()
            .find(|entry| entry.participant_id() == &participant.id())
            .map(|entry| entry.amount().value().to_string());
        let mut arguments = participant_arguments(id.as_str(), participant);
        arguments.push(Text(&position));
        arguments.push(amount.as_deref().map_or(Null, Text));
        self.statement(sql, &arguments)
    }
}
