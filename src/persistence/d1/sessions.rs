use worker::D1Type::Text;

use crate::domain::{ConfirmedSession, SessionId};
use crate::persistence::{
    Confirmation, PersistenceResult, SessionRepository, UpdateKey, UpdateRepository,
};

use super::{
    records::{corrupt, SessionRow},
    sql, D1Repository,
};

impl SessionRepository for D1Repository {
    async fn session(&self, id: &SessionId) -> PersistenceResult<Option<ConfirmedSession>> {
        self.snapshot::<SessionRow>(sql::SESSION, sql::SESSION_ENTRIES, id)
            .await?
            .map(|(header, entries)| header.decode(entries).map_err(corrupt))
            .transpose()
    }
}

impl UpdateRepository for D1Repository {
    async fn processed_update(&self, key: &UpdateKey) -> PersistenceResult<Option<Confirmation>> {
        let row = self
            .rows::<SessionRow>(sql::PROCESSED_UPDATE, &[Text(key.as_str())])
            .await?
            .into_iter()
            .next();
        row.map(|row| row.confirmation().map_err(corrupt))
            .transpose()
    }
}
