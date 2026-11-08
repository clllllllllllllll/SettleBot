use worker::{D1PreparedStatement, D1Type::Null, D1Type::Text};

use crate::persistence::{AuditChange, AuditKind, AuditRepository, PersistenceResult};

use super::D1Repository;

const ADD_AUDIT: &str = "
    INSERT INTO session_audits
        (id, session_id, revision, kind, actor_id, reason, created_at)
    VALUES (?1, (SELECT id FROM sessions WHERE id = ?2),
        COALESCE((SELECT MAX(revision) + 1 FROM session_audits WHERE session_id = ?2), 1),
        ?3, ?4, ?5, ?6)";
const ADD_AUDIT_ENTRY: &str = "
    INSERT INTO session_audit_entries
        (audit_id, kind, identity_id, roster_player_id, guest_name, position, amount_cents)
    VALUES (?1, ?2, ?3, ?4, ?5, CAST(?6 AS INTEGER), CAST(?7 AS INTEGER))";

impl AuditRepository for D1Repository {
    async fn commit_audit(&self, change: &AuditChange) -> PersistenceResult<()> {
        self.batch(self.audit_statements(change)?).await?;
        Ok(())
    }
}

impl D1Repository {
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
        self.statement(
            ADD_AUDIT,
            &[
                Text(change.key.as_str()),
                Text(change.session_id.as_str()),
                Text(change.kind.as_str()),
                Text(&actor),
                Text(change.reason.as_str()),
                Text(&created_at),
            ],
        )
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
