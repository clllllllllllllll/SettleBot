use serde::{Deserialize, Serialize};
use worker::D1Type::Text;

use crate::domain::MAX_SESSION_PARTICIPANTS;
use crate::persistence::{
    ExportCursor, ExportRepository, ExportRequest, PersistenceError, PersistenceResult,
    EXPORT_FORMAT, MAX_EXPORT_AUDITS_PER_CHUNK, MAX_EXPORT_CATALOGS_PER_CHUNK,
};

use super::D1Repository;

const PLAYERS: &str = "
    SELECT CAST(rowid AS TEXT) AS catalog_rowid, id, display_name, status FROM players
    WHERE rowid > CAST(?1 AS INTEGER) AND rowid <= CAST(?2 AS INTEGER)
    ORDER BY rowid LIMIT CAST(?3 AS INTEGER)";
const GAMES: &str = "
    SELECT CAST(rowid AS TEXT) AS catalog_rowid, id, display_name FROM games
    WHERE rowid > CAST(?1 AS INTEGER) AND rowid <= CAST(?2 AS INTEGER)
    ORDER BY rowid LIMIT CAST(?3 AS INTEGER)";
const EXPORT_SNAPSHOT: &str = "
    SELECT CAST(COALESCE((SELECT MAX(rowid) FROM players), 0) AS TEXT) AS max_player_rowid,
           CAST(COALESCE((SELECT MAX(rowid) FROM games), 0) AS TEXT) AS max_game_rowid,
           CAST(COALESCE((SELECT MAX(rowid) FROM sessions), 0) AS TEXT) AS max_session_rowid,
           CAST(COALESCE((SELECT MAX(rowid) FROM session_audits), 0) AS TEXT) AS max_audit_rowid";
const NEXT_SESSION: &str = "
    SELECT CAST(rowid AS TEXT) AS session_rowid, id, owner_id, game_id, occurred_on, created_at
    FROM sessions WHERE rowid > CAST(?1 AS INTEGER) AND rowid <= CAST(?2 AS INTEGER)
    ORDER BY rowid LIMIT 1";
const CURRENT_SESSION: &str = "
    SELECT CAST(rowid AS TEXT) AS session_rowid, id, owner_id, game_id, occurred_on, created_at
    FROM sessions WHERE rowid = CAST(?1 AS INTEGER) AND rowid <= CAST(?2 AS INTEGER)";
const HAS_NEXT_SESSION: &str = "
    SELECT EXISTS(SELECT 1 FROM sessions
      WHERE rowid > CAST(?1 AS INTEGER) AND rowid <= CAST(?2 AS INTEGER)) AS has_next";
const ENTRIES: &str = "
    SELECT kind, identity_id, roster_player_id, guest_name, position,
           CAST(amount_cents AS TEXT) AS amount_cents
    FROM session_entries WHERE session_id = ?1 ORDER BY position LIMIT 17";
const AUDITS: &str = "
    SELECT id, CAST(revision AS TEXT) AS revision, kind, actor_id, reason, created_at
    FROM session_audits WHERE session_id = ?1 AND revision > CAST(?2 AS INTEGER)
      AND rowid <= CAST(?3 AS INTEGER)
    ORDER BY session_audits.revision LIMIT CAST(?4 AS INTEGER)";
const AUDIT_ENTRIES: &str = "
    SELECT entries.audit_id, entries.kind, entries.identity_id,
           entries.roster_player_id, entries.guest_name, entries.position,
           CAST(amount_cents AS TEXT) AS amount_cents
    FROM session_audit_entries entries
    JOIN session_audits audits ON audits.id = entries.audit_id
    WHERE audits.session_id = ?1 AND audits.revision > CAST(?2 AS INTEGER)
      AND audits.revision <= CAST(?3 AS INTEGER) AND audits.rowid <= CAST(?4 AS INTEGER)
    ORDER BY audits.revision, entries.position LIMIT CAST(?5 AS INTEGER)";

#[derive(Deserialize, Serialize)]
struct PlayerRow {
    #[serde(skip_serializing)]
    catalog_rowid: String,
    id: String,
    display_name: String,
    status: String,
}

#[derive(Deserialize, Serialize)]
struct GameRow {
    #[serde(skip_serializing)]
    catalog_rowid: String,
    id: String,
    display_name: String,
}

#[derive(Deserialize, Serialize)]
struct EntryRow {
    #[serde(default, skip_serializing)]
    audit_id: String,
    kind: String,
    identity_id: String,
    roster_player_id: Option<String>,
    guest_name: Option<String>,
    position: u32,
    amount_cents: String,
}

#[derive(Deserialize, Serialize)]
struct AuditRow {
    id: String,
    revision: String,
    kind: String,
    actor_id: String,
    reason: String,
    created_at: String,
    #[serde(default)]
    entries: Vec<EntryRow>,
}

#[derive(Deserialize, Serialize)]
struct SessionRow {
    #[serde(skip_serializing)]
    session_rowid: String,
    id: String,
    owner_id: String,
    game_id: String,
    occurred_on: String,
    created_at: String,
    #[serde(default)]
    entries: Vec<EntryRow>,
    #[serde(default)]
    audits: Vec<AuditRow>,
}

#[derive(Deserialize)]
struct SnapshotRow {
    max_player_rowid: String,
    max_game_rowid: String,
    max_session_rowid: String,
    max_audit_rowid: String,
}

#[derive(Deserialize)]
struct ExistsRow {
    has_next: u8,
}

#[derive(Serialize)]
struct ExportPage {
    format: &'static str,
    next_cursor: Option<String>,
    players: Vec<PlayerRow>,
    games: Vec<GameRow>,
    sessions: Vec<SessionRow>,
}

impl ExportRepository for D1Repository {
    async fn export_json(&self, request: &ExportRequest) -> PersistenceResult<String> {
        let page = self.export_page(request).await?;
        serde_json::to_string(&page).map_err(|_| PersistenceError::Corrupt("export encoding"))
    }
}

impl D1Repository {
    async fn export_page(&self, request: &ExportRequest) -> PersistenceResult<ExportPage> {
        let cursor = self.export_cursor(request).await?;
        let (players, player_rowid) = self.export_players(&cursor).await?;
        let (games, game_rowid) = self.export_games(&cursor).await?;
        let cursor = cursor.next_catalogs(player_rowid, game_rowid);
        if !cursor.catalogs_complete() {
            return Ok(export_page(
                players,
                games,
                Vec::new(),
                Some(cursor.encode()),
            ));
        }
        let Some(mut session) = self.export_session(&cursor).await? else {
            return Ok(export_page(players, games, Vec::new(), None));
        };
        let next_cursor = self.fill_session(&mut session, &cursor).await?;
        Ok(export_page(
            players,
            games,
            vec![session],
            next_cursor.map(|cursor| cursor.encode()),
        ))
    }

    async fn export_players(
        &self,
        cursor: &ExportCursor,
    ) -> PersistenceResult<(Vec<PlayerRow>, i64)> {
        let after = cursor.player_rowid().to_string();
        let maximum = cursor.max_player_rowid().to_string();
        let limit = (MAX_EXPORT_CATALOGS_PER_CHUNK + 1).to_string();
        let rows = self
            .rows(PLAYERS, &[Text(&after), Text(&maximum), Text(&limit)])
            .await?;
        player_catalog_page(rows, cursor.max_player_rowid())
    }

    async fn export_games(&self, cursor: &ExportCursor) -> PersistenceResult<(Vec<GameRow>, i64)> {
        let after = cursor.game_rowid().to_string();
        let maximum = cursor.max_game_rowid().to_string();
        let limit = (MAX_EXPORT_CATALOGS_PER_CHUNK + 1).to_string();
        let rows = self
            .rows(GAMES, &[Text(&after), Text(&maximum), Text(&limit)])
            .await?;
        game_catalog_page(rows, cursor.max_game_rowid())
    }

    async fn export_cursor(&self, request: &ExportRequest) -> PersistenceResult<ExportCursor> {
        if let Some(cursor) = request.cursor() {
            return Ok(cursor.clone());
        }
        let row = self
            .rows::<SnapshotRow>(EXPORT_SNAPSHOT, &[])
            .await?
            .into_iter()
            .next()
            .ok_or(PersistenceError::Corrupt("export snapshot"))?;
        ExportCursor::snapshot(
            parse_rowid(&row.max_player_rowid)?,
            parse_rowid(&row.max_game_rowid)?,
            parse_rowid(&row.max_session_rowid)?,
            parse_rowid(&row.max_audit_rowid)?,
        )
    }

    async fn export_session(&self, cursor: &ExportCursor) -> PersistenceResult<Option<SessionRow>> {
        let current = cursor.session_rowid().to_string();
        let maximum = cursor.max_session_rowid().to_string();
        let query = if cursor.after_revision() == 0 {
            NEXT_SESSION
        } else {
            CURRENT_SESSION
        };
        let session = self
            .rows(query, &[Text(&current), Text(&maximum)])
            .await?
            .into_iter()
            .next();
        if cursor.after_revision() > 0 && session.is_none() {
            return Err(PersistenceError::InvalidExportCursor);
        }
        Ok(session)
    }

    async fn fill_session(
        &self,
        session: &mut SessionRow,
        cursor: &ExportCursor,
    ) -> PersistenceResult<Option<ExportCursor>> {
        if cursor.after_revision() == 0 {
            session.entries = self.export_entries(&session.id).await?;
        }
        let more_audits = self.export_audits(session, cursor).await?;
        let rowid = parse_rowid(&session.session_rowid)?;
        if more_audits {
            let revision = last_audit_revision(&session.audits)?;
            return Ok(Some(cursor.next_audit(rowid, revision)));
        }
        self.next_session_cursor(cursor, rowid).await
    }

    async fn export_entries(&self, session: &str) -> PersistenceResult<Vec<EntryRow>> {
        let entries = self.rows(ENTRIES, &[Text(session)]).await?;
        if entries.len() > MAX_SESSION_PARTICIPANTS {
            return Err(PersistenceError::Corrupt("export session entries"));
        }
        Ok(entries)
    }

    async fn export_audits(
        &self,
        session: &mut SessionRow,
        cursor: &ExportCursor,
    ) -> PersistenceResult<bool> {
        let after = cursor.after_revision().to_string();
        let maximum = cursor.max_audit_rowid().to_string();
        let limit = (MAX_EXPORT_AUDITS_PER_CHUNK + 1).to_string();
        let arguments = [
            Text(&session.id),
            Text(&after),
            Text(&maximum),
            Text(&limit),
        ];
        session.audits = self.rows(AUDITS, &arguments).await?;
        let has_more = session.audits.len() > MAX_EXPORT_AUDITS_PER_CHUNK;
        session.audits.truncate(MAX_EXPORT_AUDITS_PER_CHUNK);
        self.fill_audit_entries(session, cursor).await?;
        Ok(has_more)
    }

    async fn fill_audit_entries(
        &self,
        session: &mut SessionRow,
        cursor: &ExportCursor,
    ) -> PersistenceResult<()> {
        let Some(last) = session.audits.last() else {
            return Ok(());
        };
        let entries = self
            .export_audit_entries(&session.id, &last.revision, cursor)
            .await?;
        if entries.len() > MAX_EXPORT_AUDITS_PER_CHUNK * MAX_SESSION_PARTICIPANTS {
            return Err(PersistenceError::Corrupt("export audit entries"));
        }
        attach_audit_entries(&mut session.audits, entries)
    }

    async fn export_audit_entries(
        &self,
        session: &str,
        last_revision: &str,
        cursor: &ExportCursor,
    ) -> PersistenceResult<Vec<EntryRow>> {
        let after = cursor.after_revision().to_string();
        let maximum = cursor.max_audit_rowid().to_string();
        let limit = (MAX_EXPORT_AUDITS_PER_CHUNK * MAX_SESSION_PARTICIPANTS + 1).to_string();
        let arguments = [
            Text(session),
            Text(&after),
            Text(last_revision),
            Text(&maximum),
            Text(&limit),
        ];
        self.rows(AUDIT_ENTRIES, &arguments).await
    }

    async fn next_session_cursor(
        &self,
        cursor: &ExportCursor,
        rowid: i64,
    ) -> PersistenceResult<Option<ExportCursor>> {
        let current = rowid.to_string();
        let maximum = cursor.max_session_rowid().to_string();
        let exists = self
            .rows::<ExistsRow>(HAS_NEXT_SESSION, &[Text(&current), Text(&maximum)])
            .await?
            .first()
            .is_some_and(|row| row.has_next == 1);
        Ok(exists.then(|| cursor.next_session(rowid)))
    }
}

fn export_page(
    players: Vec<PlayerRow>,
    games: Vec<GameRow>,
    sessions: Vec<SessionRow>,
    next_cursor: Option<String>,
) -> ExportPage {
    ExportPage {
        format: EXPORT_FORMAT,
        next_cursor,
        players,
        games,
        sessions,
    }
}

fn last_audit_revision(audits: &[AuditRow]) -> PersistenceResult<u64> {
    audits
        .last()
        .ok_or(PersistenceError::Corrupt("export audit page"))?
        .revision
        .parse()
        .map_err(|_| PersistenceError::Corrupt("export audit revision"))
}

fn player_catalog_page(
    mut rows: Vec<PlayerRow>,
    maximum: i64,
) -> PersistenceResult<(Vec<PlayerRow>, i64)> {
    let next = catalog_next_rowid(&rows, maximum)?;
    rows.truncate(MAX_EXPORT_CATALOGS_PER_CHUNK);
    Ok((rows, next))
}

fn game_catalog_page(
    mut rows: Vec<GameRow>,
    maximum: i64,
) -> PersistenceResult<(Vec<GameRow>, i64)> {
    let next = catalog_next_rowid(&rows, maximum)?;
    rows.truncate(MAX_EXPORT_CATALOGS_PER_CHUNK);
    Ok((rows, next))
}

fn catalog_next_rowid<T: CatalogRow>(rows: &[T], maximum: i64) -> PersistenceResult<i64> {
    if rows.len() <= MAX_EXPORT_CATALOGS_PER_CHUNK {
        return Ok(maximum);
    }
    parse_rowid(rows[MAX_EXPORT_CATALOGS_PER_CHUNK - 1].catalog_rowid())
}

trait CatalogRow {
    fn catalog_rowid(&self) -> &str;
}

impl CatalogRow for PlayerRow {
    fn catalog_rowid(&self) -> &str {
        &self.catalog_rowid
    }
}

impl CatalogRow for GameRow {
    fn catalog_rowid(&self) -> &str {
        &self.catalog_rowid
    }
}

fn attach_audit_entries(audits: &mut [AuditRow], entries: Vec<EntryRow>) -> PersistenceResult<()> {
    for entry in entries {
        let audit = audits
            .iter_mut()
            .find(|audit| audit.id == entry.audit_id)
            .ok_or(PersistenceError::Corrupt("export audit entry"))?;
        audit.entries.push(entry);
    }
    if audits
        .iter()
        .any(|audit| audit.entries.len() > MAX_SESSION_PARTICIPANTS)
    {
        return Err(PersistenceError::Corrupt("export audit entries"));
    }
    Ok(())
}

fn parse_rowid(value: &str) -> PersistenceResult<i64> {
    value
        .parse()
        .map_err(|_| PersistenceError::Corrupt("export rowid"))
}
