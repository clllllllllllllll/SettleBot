use worker::{D1PreparedStatement, D1Type::Text};

use crate::domain::ConfirmedSession;
use crate::persistence::{
    Confirmation, ConfirmationRepository, OwnerId, PersistenceResult, Revision, UpdateKey,
};

use super::{sql, D1Repository};

impl ConfirmationRepository for D1Repository {
    async fn commit_confirmation(
        &self,
        key: &UpdateKey,
        owner: OwnerId,
        revision: Revision,
        session: &ConfirmedSession,
    ) -> PersistenceResult<Confirmation> {
        let mut statements = vec![self.confirm_header(owner, revision, session)?];
        self.confirm_entries(&mut statements, session)?;
        statements.push(self.statement(sql::CONSUME_DRAFT, &[Text(session.id().as_str())])?);
        statements.push(self.statement(
            sql::PROCESS_UPDATE,
            &[Text(key.as_str()), Text(session.id().as_str())],
        )?);
        self.batch(statements).await?;
        Ok(Confirmation {
            session_id: session.id().clone(),
            owner,
            created_at: session.created_at(),
        })
    }
}

impl D1Repository {
    fn confirm_header(
        &self,
        owner: OwnerId,
        revision: Revision,
        session: &ConfirmedSession,
    ) -> PersistenceResult<D1PreparedStatement> {
        let owner = owner.value().to_string();
        let revision = revision.value().to_string();
        let created_at = session.created_at().to_string();
        self.statement(
            sql::CONFIRM_SESSION,
            &[
                Text(session.id().as_str()),
                Text(&owner),
                Text(&revision),
                Text(&created_at),
            ],
        )
    }

    fn confirm_entries(
        &self,
        statements: &mut Vec<D1PreparedStatement>,
        session: &ConfirmedSession,
    ) -> PersistenceResult<()> {
        for (position, participant) in session.participants().iter().enumerate() {
            statements.push(self.participant_statement(
                sql::ADD_SESSION_ENTRY,
                session.id(),
                position,
                participant,
                session.entries(),
            )?);
        }
        Ok(())
    }
}
