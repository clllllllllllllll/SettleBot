use super::{PersistenceError, PersistenceResult};

pub const EXPORT_FORMAT: &str = "settlebot-ledger-v1";
pub const MAX_EXPORT_CATALOGS_PER_CHUNK: usize = 24;
pub const MAX_EXPORT_AUDITS_PER_CHUNK: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportCursor {
    max_player_rowid: i64,
    max_game_rowid: i64,
    max_session_rowid: i64,
    max_audit_rowid: i64,
    player_rowid: i64,
    game_rowid: i64,
    session_rowid: i64,
    after_revision: u64,
}

impl ExportCursor {
    pub fn parse(value: &str) -> PersistenceResult<Self> {
        let values: Vec<_> = value.split('.').collect();
        if values.len() != 8 {
            return Err(PersistenceError::InvalidExportCursor);
        }
        let cursor = Self {
            max_player_rowid: parse_i64(values[0])?,
            max_game_rowid: parse_i64(values[1])?,
            max_session_rowid: parse_i64(values[2])?,
            max_audit_rowid: parse_i64(values[3])?,
            player_rowid: parse_i64(values[4])?,
            game_rowid: parse_i64(values[5])?,
            session_rowid: parse_i64(values[6])?,
            after_revision: values[7]
                .parse()
                .map_err(|_| PersistenceError::InvalidExportCursor)?,
        };
        cursor.validate()?;
        Ok(cursor)
    }

    pub fn encode(&self) -> String {
        format!(
            "{}.{}.{}.{}.{}.{}.{}.{}",
            self.max_player_rowid,
            self.max_game_rowid,
            self.max_session_rowid,
            self.max_audit_rowid,
            self.player_rowid,
            self.game_rowid,
            self.session_rowid,
            self.after_revision
        )
    }

    pub(crate) fn snapshot(
        max_player_rowid: i64,
        max_game_rowid: i64,
        max_session_rowid: i64,
        max_audit_rowid: i64,
    ) -> PersistenceResult<Self> {
        let cursor = Self {
            max_player_rowid,
            max_game_rowid,
            max_session_rowid,
            max_audit_rowid,
            player_rowid: 0,
            game_rowid: 0,
            session_rowid: 0,
            after_revision: 0,
        };
        cursor.validate()?;
        Ok(cursor)
    }

    pub(crate) fn next_audit(&self, session_rowid: i64, revision: u64) -> Self {
        Self {
            session_rowid,
            after_revision: revision,
            ..self.clone()
        }
    }

    pub(crate) fn next_catalogs(&self, player_rowid: i64, game_rowid: i64) -> Self {
        Self {
            player_rowid,
            game_rowid,
            ..self.clone()
        }
    }

    pub(crate) fn next_session(&self, session_rowid: i64) -> Self {
        Self {
            session_rowid,
            after_revision: 0,
            ..self.clone()
        }
    }

    pub(crate) fn max_session_rowid(&self) -> i64 {
        self.max_session_rowid
    }

    pub(crate) fn max_player_rowid(&self) -> i64 {
        self.max_player_rowid
    }

    pub(crate) fn max_game_rowid(&self) -> i64 {
        self.max_game_rowid
    }

    pub(crate) fn max_audit_rowid(&self) -> i64 {
        self.max_audit_rowid
    }

    pub(crate) fn player_rowid(&self) -> i64 {
        self.player_rowid
    }

    pub(crate) fn game_rowid(&self) -> i64 {
        self.game_rowid
    }

    pub(crate) fn catalogs_complete(&self) -> bool {
        self.player_rowid == self.max_player_rowid && self.game_rowid == self.max_game_rowid
    }

    pub(crate) fn session_rowid(&self) -> i64 {
        self.session_rowid
    }

    pub(crate) fn after_revision(&self) -> u64 {
        self.after_revision
    }

    fn validate(&self) -> PersistenceResult<()> {
        if self.max_player_rowid < 0
            || self.max_game_rowid < 0
            || self.max_session_rowid < 0
            || self.max_audit_rowid < 0
            || self.player_rowid < 0
            || self.player_rowid > self.max_player_rowid
            || self.game_rowid < 0
            || self.game_rowid > self.max_game_rowid
            || self.session_rowid < 0
            || self.session_rowid > self.max_session_rowid
        {
            return Err(PersistenceError::InvalidExportCursor);
        }
        Ok(())
    }
}

fn parse_i64(value: &str) -> PersistenceResult<i64> {
    value
        .parse()
        .map_err(|_| PersistenceError::InvalidExportCursor)
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExportRequest {
    cursor: Option<ExportCursor>,
}

impl ExportRequest {
    pub fn first() -> Self {
        Self::default()
    }

    pub fn resume(cursor: &str) -> PersistenceResult<Self> {
        Ok(Self {
            cursor: Some(ExportCursor::parse(cursor)?),
        })
    }

    pub(crate) fn cursor(&self) -> Option<&ExportCursor> {
        self.cursor.as_ref()
    }
}

#[allow(async_fn_in_trait)]
pub trait ExportRepository {
    /// Return one bounded JSON chunk from a stable append-only snapshot.
    async fn export_json(&self, request: &ExportRequest) -> PersistenceResult<String>;
}
