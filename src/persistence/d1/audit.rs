use worker::{D1PreparedStatement, D1Type::Null, D1Type::Text};

use crate::persistence::{
    AuditChange, AuditKind, AuditRepository, AuditTarget, PersistenceError, PersistenceResult,
};

use super::D1Repository;

const ADD_AUDIT: &str = "
    INSERT INTO session_audits
        (id, session_id, revision, kind, actor_id, reason, created_at)
    VALUES (?1, (SELECT id FROM sessions WHERE id = ?2
        AND COALESCE((SELECT MAX(revision) FROM session_audits WHERE session_id = ?2), 0) = CAST(?7 AS INTEGER)
        AND COALESCE((SELECT kind FROM session_audits WHERE session_id = ?2 ORDER BY revision DESC LIMIT 1), 'original') = ?8
        AND ?8 != 'void'),
        CAST(?7 AS INTEGER) + 1,
        ?3, ?4, ?5, ?6)";
const AUDIT_PRECONDITION: &str = "
    SELECT EXISTS (SELECT 1 FROM sessions WHERE id = ?1
      AND COALESCE((SELECT MAX(revision) FROM session_audits WHERE session_id = ?1), 0) = CAST(?2 AS INTEGER)
      AND COALESCE((SELECT kind FROM session_audits WHERE session_id = ?1 ORDER BY revision DESC LIMIT 1), 'original') = ?3
      AND ?3 != 'void') AS valid";

#[derive(serde::Deserialize)]
struct PreconditionRow {
    valid: u8,
}
const ADD_AUDIT_ENTRY: &str = "
    INSERT INTO session_audit_entries
        (audit_id, kind, identity_id, roster_player_id, guest_name, position, amount_cents)
    VALUES (?1, ?2, ?3, ?4, ?5, CAST(?6 AS INTEGER), CAST(?7 AS INTEGER))";

impl AuditRepository for D1Repository {
    async fn commit_audit(&self, change: &AuditChange) -> PersistenceResult<()> {
        match self.batch(self.audit_statements(change)?).await {
            Ok(_) => Ok(()),
            Err(error) => Err(self.audit_failure(change, error).await),
        }
    }
}

impl D1Repository {
    pub(crate) async fn audit_failure(
        &self,
        change: &AuditChange,
        failure: PersistenceError,
    ) -> PersistenceError {
        match self.audit_precondition_matches(&change.target).await {
            Ok(false) => PersistenceError::AuditConflict,
            _ => failure,
        }
    }

    async fn audit_precondition_matches(&self, target: &AuditTarget) -> PersistenceResult<bool> {
        let revision = target.revision().value().to_string();
        let rows = self
            .rows::<PreconditionRow>(
                AUDIT_PRECONDITION,
                &[
                    Text(target.session_id().as_str()),
                    Text(&revision),
                    Text(target.prior_kind()),
                ],
            )
            .await?;
        rows.first()
            .map(|row| row.valid != 0)
            .ok_or(PersistenceError::Corrupt("audit precondition"))
    }
    pub(crate) fn audit_statements(
        &self,
        change: &AuditChange,
    ) -> PersistenceResult<Vec<D1PreparedStatement>> {
        let mut statements = vec![self.audit_header_statement(change)?];
        if change.kind == AuditKind::Correction {
            statements.extend(self.audit_entry_statements(change)?);
        }
        Ok(statements)
    }

    fn audit_header_statement(
        &self,
        change: &AuditChange,
    ) -> PersistenceResult<D1PreparedStatement> {
        let actor = change.actor.value().to_string();
        let created_at = change.created_at.to_string();
        let revision = change.target.revision().value().to_string();
        let values = [
            Text(change.key.as_str()),
            Text(change.target.session_id().as_str()),
            Text(change.kind.as_str()),
            Text(&actor),
            Text(change.reason.as_str()),
            Text(&created_at),
            Text(&revision),
            Text(change.target.prior_kind()),
        ];
        self.statement(ADD_AUDIT, &values)
    }

    fn audit_entry_statements(
        &self,
        change: &AuditChange,
    ) -> PersistenceResult<Vec<D1PreparedStatement>> {
        change
            .entries
            .iter()
            .enumerate()
            .map(|(position, entry)| {
                let position = position.to_string();
                let amount = entry.amount.value().to_string();
                let (kind, roster, guest) = match entry.roster_id() {
                    Some(player) => ("roster", Text(player.as_str()), Null),
                    None => ("guest", Null, Text(entry.display_name.as_str())),
                };
                self.statement(
                    ADD_AUDIT_ENTRY,
                    &[
                        Text(change.key.as_str()),
                        Text(kind),
                        Text(entry.identity_id()),
                        roster,
                        guest,
                        Text(&position),
                        Text(&amount),
                    ],
                )
            })
            .collect()
    }
}
