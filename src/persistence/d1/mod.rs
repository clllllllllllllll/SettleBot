//! Cloudflare D1 adapter. Money and numeric IDs cross the JS boundary as decimal
//! text; SQLite stores cents as INTEGER without JavaScript's 53-bit rounding.

mod catalog;
mod confirmation;
mod drafts;
mod records;
mod sessions;
mod sql;

use serde::de::DeserializeOwned;
use worker::{D1Database, D1PreparedStatement, D1Result, D1Type};

use super::{PersistenceError, PersistenceResult};
use crate::domain::SessionId;
use records::ParticipantRow;

pub const DATABASE_BINDING: &str = "LEDGER_DB";

pub struct D1Repository {
    database: D1Database,
}

impl D1Repository {
    pub fn new(database: D1Database) -> Self {
        Self { database }
    }

    fn statement(
        &self,
        sql: &str,
        arguments: &[D1Type<'_>],
    ) -> PersistenceResult<D1PreparedStatement> {
        self.database
            .prepare(sql)
            .bind_refs(arguments)
            .map_err(storage_error)
    }

    async fn rows<T: DeserializeOwned>(
        &self,
        sql: &str,
        arguments: &[D1Type<'_>],
    ) -> PersistenceResult<Vec<T>> {
        self.statement(sql, arguments)?
            .all()
            .await
            .map_err(storage_error)?
            .results()
            .map_err(|_| PersistenceError::Corrupt("row encoding"))
    }

    async fn batch(
        &self,
        statements: Vec<D1PreparedStatement>,
    ) -> PersistenceResult<Vec<D1Result>> {
        let results = self
            .database
            .batch(statements)
            .await
            .map_err(storage_error)?;
        if results.iter().any(|result| !result.success()) {
            return Err(PersistenceError::Storage);
        }
        Ok(results)
    }

    async fn insert_identity(&self, sql: &str, arguments: &[D1Type<'_>]) -> PersistenceResult<()> {
        let result = self
            .statement(sql, arguments)?
            .run()
            .await
            .map_err(storage_error)?;
        if result
            .meta()
            .map_err(storage_error)?
            .and_then(|meta| meta.changes)
            != Some(1)
        {
            return Err(PersistenceError::IdentityConflict);
        }
        Ok(())
    }

    async fn snapshot<T: DeserializeOwned>(
        &self,
        header_query: &str,
        participants_query: &str,
        id: &SessionId,
    ) -> PersistenceResult<Option<(T, Vec<ParticipantRow>)>> {
        let arguments = [D1Type::Text(id.as_str())];
        let statements = vec![
            self.statement(header_query, &arguments)?,
            self.statement(participants_query, &arguments)?,
        ];
        decode_snapshot(self.batch(statements).await?)
    }
}

fn decode_snapshot<T: DeserializeOwned>(
    results: Vec<D1Result>,
) -> PersistenceResult<Option<(T, Vec<ParticipantRow>)>> {
    let mut results = results.into_iter();
    let header = results
        .next()
        .ok_or(PersistenceError::Corrupt("header query"))?;
    let rows = results
        .next()
        .ok_or(PersistenceError::Corrupt("participants query"))?;
    let header = decode_rows::<T>(header)?.into_iter().next();
    header
        .map(|header| Ok((header, decode_rows(rows)?)))
        .transpose()
}

fn storage_error(_: worker::Error) -> PersistenceError {
    PersistenceError::Storage
}

fn decode_rows<T: DeserializeOwned>(result: D1Result) -> PersistenceResult<Vec<T>> {
    result
        .results()
        .map_err(|_| PersistenceError::Corrupt("row encoding"))
}
