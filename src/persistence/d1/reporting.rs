use std::collections::BTreeMap;

use serde::{de::DeserializeOwned, Deserialize};
use worker::{D1Result, D1Type::Text};

use crate::domain::{
    Cents, CreatedAt, DisplayName, GameId, GuestId, OccurredOn, PlayerId, SessionId,
};
use crate::persistence::{
    AuditKind, AuditPageRequest, AuditReason, AuditedSessionState, GameLeaderboard,
    LeaderboardEntry, OwnerId, PersistenceError, PersistenceResult, PlayerReport,
    PlayerSessionSummary, ReportPage, ReportPageRequest, ReportParticipantIdentity, ReportTotal,
    ReportingRepository, SessionAudit, SessionAuditPage, SessionDetail, SessionDetailEntry,
    SessionSummary,
};

use super::{decode_rows, records::corrupt, D1Repository};
use crate::persistence::report_scan::{scan, BatchSource, REPORT_CONTRIBUTION_BATCH_SIZE};

const REPORT_ROSTER: &str = "
    SELECT id AS player_id, display_name AS player_name FROM players ORDER BY id";
const REPORT_SNAPSHOT: &str = "
    SELECT CAST(COALESCE((SELECT MAX(rowid) FROM sessions), 0) AS TEXT) AS max_session_rowid,
           CAST(COALESCE((SELECT MAX(rowid) FROM session_audits), 0) AS TEXT) AS max_audit_rowid";
const OVERALL_CONTRIBUTIONS: &str = "
    WITH effective AS (
      SELECT e.roster_player_id, e.session_id, e.amount_cents
      FROM session_entries e JOIN sessions s ON s.id = e.session_id
      WHERE s.rowid <= CAST(?1 AS INTEGER) AND NOT EXISTS (
        SELECT 1 FROM session_audits a
        WHERE a.session_id = s.id AND a.rowid <= CAST(?2 AS INTEGER))
      UNION ALL
      SELECT e.roster_player_id, a.session_id, e.amount_cents
      FROM session_audit_entries e JOIN session_audits a ON a.id = e.audit_id
      JOIN sessions s ON s.id = a.session_id
      WHERE s.rowid <= CAST(?1 AS INTEGER) AND a.kind = 'correction'
        AND a.rowid = (SELECT MAX(latest.rowid) FROM session_audits latest
          WHERE latest.session_id = a.session_id AND latest.rowid <= CAST(?2 AS INTEGER))
    )
    SELECT roster_player_id AS player_id, session_id,
           CAST(amount_cents AS TEXT) AS amount_cents FROM effective
    WHERE roster_player_id IS NOT NULL AND (roster_player_id > ?3
      OR (roster_player_id = ?3 AND session_id > ?4))
    ORDER BY roster_player_id, session_id LIMIT CAST(?5 AS INTEGER)";
const GAME: &str = "SELECT display_name FROM games WHERE id = ?";
const GAME_CONTRIBUTIONS: &str = "
    WITH effective AS (
      SELECT e.roster_player_id, e.session_id, e.amount_cents
      FROM session_entries e JOIN sessions s ON s.id = e.session_id
      WHERE s.game_id = ?1 AND s.rowid <= CAST(?2 AS INTEGER) AND NOT EXISTS (
        SELECT 1 FROM session_audits a
        WHERE a.session_id = s.id AND a.rowid <= CAST(?3 AS INTEGER))
      UNION ALL
      SELECT e.roster_player_id, a.session_id, e.amount_cents
      FROM session_audit_entries e JOIN session_audits a ON a.id = e.audit_id
      JOIN sessions s ON s.id = a.session_id
      WHERE s.game_id = ?1 AND s.rowid <= CAST(?2 AS INTEGER) AND a.kind = 'correction'
        AND a.rowid = (SELECT MAX(latest.rowid) FROM session_audits latest
          WHERE latest.session_id = a.session_id AND latest.rowid <= CAST(?3 AS INTEGER))
    )
    SELECT roster_player_id AS player_id, session_id,
           CAST(amount_cents AS TEXT) AS amount_cents FROM effective
    WHERE roster_player_id IS NOT NULL AND (roster_player_id > ?4
      OR (roster_player_id = ?4 AND session_id > ?5))
    ORDER BY roster_player_id, session_id LIMIT CAST(?6 AS INTEGER)";
const PLAYER: &str = "
    SELECT id AS player_id, display_name AS player_name FROM players WHERE id = ?";
const PLAYER_CONTRIBUTIONS: &str = "
    WITH effective AS (
      SELECT e.roster_player_id, e.session_id, e.amount_cents
      FROM session_entries e JOIN sessions s ON s.id = e.session_id
      WHERE s.rowid <= CAST(?2 AS INTEGER) AND NOT EXISTS (
        SELECT 1 FROM session_audits a
        WHERE a.session_id = s.id AND a.rowid <= CAST(?3 AS INTEGER))
      UNION ALL
      SELECT e.roster_player_id, a.session_id, e.amount_cents
      FROM session_audit_entries e JOIN session_audits a ON a.id = e.audit_id
      JOIN sessions s ON s.id = a.session_id
      WHERE s.rowid <= CAST(?2 AS INTEGER) AND a.kind = 'correction'
        AND a.rowid = (SELECT MAX(latest.rowid) FROM session_audits latest
          WHERE latest.session_id = a.session_id AND latest.rowid <= CAST(?3 AS INTEGER))
    )
    SELECT session_id, CAST(amount_cents AS TEXT) AS amount_cents FROM effective
    WHERE roster_player_id = ?1 AND session_id > ?4
    ORDER BY session_id LIMIT CAST(?5 AS INTEGER)";
const PLAYER_SESSIONS: &str = "
    WITH effective AS (
      SELECT e.roster_player_id, e.session_id, e.amount_cents
      FROM session_entries e JOIN sessions s ON s.id = e.session_id
      WHERE NOT EXISTS (SELECT 1 FROM session_audits a WHERE a.session_id = s.id)
      UNION ALL
      SELECT e.roster_player_id, a.session_id, e.amount_cents
      FROM session_audit_entries e JOIN session_audits a ON a.id = e.audit_id
      WHERE a.kind = 'correction' AND a.rowid = (
        SELECT MAX(latest.rowid) FROM session_audits latest WHERE latest.session_id = a.session_id)
    )
    SELECT s.id AS session_id, games.display_name AS game_name,
           s.occurred_on, CAST(e.amount_cents AS TEXT) AS amount_cents
    FROM effective e JOIN sessions s ON s.id = e.session_id
    JOIN games ON games.id = s.game_id WHERE e.roster_player_id = ?1
    ORDER BY s.occurred_on DESC, s.id DESC
    LIMIT CAST(?2 AS INTEGER) OFFSET CAST(?3 AS INTEGER)";
#[cfg(feature = "d1-test")]
const PLAYER_SESSIONS_AT_SNAPSHOT: &str = "
    WITH effective AS (
      SELECT e.roster_player_id, e.session_id, e.amount_cents
      FROM session_entries e JOIN sessions s ON s.id = e.session_id
      WHERE s.rowid <= CAST(?2 AS INTEGER) AND NOT EXISTS (
        SELECT 1 FROM session_audits a WHERE a.session_id = s.id AND a.rowid <= CAST(?3 AS INTEGER))
      UNION ALL
      SELECT e.roster_player_id, a.session_id, e.amount_cents
      FROM session_audit_entries e JOIN session_audits a ON a.id = e.audit_id
      JOIN sessions s ON s.id = a.session_id
      WHERE s.rowid <= CAST(?2 AS INTEGER) AND a.kind = 'correction' AND a.rowid = (
        SELECT MAX(latest.rowid) FROM session_audits latest
        WHERE latest.session_id = a.session_id AND latest.rowid <= CAST(?3 AS INTEGER))
    )
    SELECT s.id AS session_id, games.display_name AS game_name,
           s.occurred_on, CAST(e.amount_cents AS TEXT) AS amount_cents
    FROM effective e JOIN sessions s ON s.id = e.session_id
    JOIN games ON games.id = s.game_id WHERE e.roster_player_id = ?1
    ORDER BY s.occurred_on DESC, s.id DESC
    LIMIT CAST(?4 AS INTEGER) OFFSET CAST(?5 AS INTEGER)";
const SESSION_HISTORY: &str = "
    SELECT sessions.id AS session_id, games.display_name AS game_name, sessions.occurred_on
    FROM sessions JOIN games ON games.id = sessions.game_id
    WHERE COALESCE((SELECT kind FROM session_audits
      WHERE session_id = sessions.id ORDER BY rowid DESC LIMIT 1), 'original') <> 'void'
    ORDER BY sessions.occurred_on DESC, sessions.id DESC
    LIMIT CAST(?1 AS INTEGER) OFFSET CAST(?2 AS INTEGER)";
const SESSION_HEADER: &str = "
    SELECT sessions.id AS session_id, games.display_name AS game_name, sessions.occurred_on
    FROM sessions JOIN games ON games.id = sessions.game_id WHERE sessions.id = ?";
const SESSION_DETAIL_ENTRIES: &str = "
    WITH latest_correction AS (SELECT id FROM session_audits
      WHERE session_id = ?1 AND kind = 'correction' ORDER BY rowid DESC LIMIT 1),
    effective AS (
      SELECT kind, identity_id, roster_player_id, guest_name, position, amount_cents
      FROM session_entries WHERE session_id = ?1 AND NOT EXISTS (SELECT 1 FROM latest_correction)
      UNION ALL
      SELECT kind, identity_id, roster_player_id, guest_name, position, amount_cents
      FROM session_audit_entries WHERE audit_id = (SELECT id FROM latest_correction)
    )
    SELECT e.kind, e.identity_id,
      CASE WHEN e.kind = 'roster' THEN players.display_name ELSE e.guest_name END AS display_name,
      CAST(e.amount_cents AS TEXT) AS amount_cents FROM effective e
    LEFT JOIN players ON players.id = e.roster_player_id ORDER BY e.position";
const SESSION_AUDITS: &str = "
    SELECT CAST(revision AS TEXT) AS revision, kind, actor_id, reason, created_at
    FROM session_audits WHERE session_id = ?1 AND revision > CAST(?2 AS INTEGER)
    ORDER BY session_audits.revision LIMIT CAST(?3 AS INTEGER)";
const SESSION_AUDIT_STATE: &str = "
    SELECT kind FROM session_audits WHERE session_id = ? ORDER BY revision DESC LIMIT 1";

#[derive(Deserialize)]
struct RosterRow {
    player_id: String,
    player_name: String,
}

impl RosterRow {
    fn decode(self) -> PersistenceResult<(PlayerId, DisplayName)> {
        Ok((
            PlayerId::new(&self.player_id)?,
            DisplayName::new(&self.player_name)?,
        ))
    }
}

#[derive(Deserialize)]
struct ContributionRow {
    player_id: String,
    session_id: String,
    amount_cents: String,
}

impl ContributionRow {
    fn decode(&self) -> PersistenceResult<(PlayerId, Cents)> {
        Ok((
            PlayerId::new(&self.player_id)?,
            decode_cents(&self.amount_cents)?,
        ))
    }
}

#[derive(Deserialize)]
struct AmountRow {
    session_id: String,
    amount_cents: String,
}

#[derive(Deserialize)]
struct SnapshotRow {
    max_session_rowid: String,
    max_audit_rowid: String,
}

#[derive(Clone)]
struct ReportSnapshot {
    sessions: String,
    audits: String,
}

impl ReportSnapshot {
    fn decode(row: SnapshotRow) -> PersistenceResult<Self> {
        let sessions = snapshot_value(&row.max_session_rowid)?;
        let audits = snapshot_value(&row.max_audit_rowid)?;
        if sessions < 0 || audits < 0 {
            return Err(PersistenceError::Corrupt("report snapshot"));
        }
        Ok(Self {
            sessions: row.max_session_rowid,
            audits: row.max_audit_rowid,
        })
    }

    #[cfg(feature = "d1-test")]
    fn from_text(value: &str) -> PersistenceResult<Self> {
        let (sessions, audits) = value
            .split_once(':')
            .ok_or(PersistenceError::Corrupt("report snapshot"))?;
        Self::decode(SnapshotRow {
            max_session_rowid: sessions.to_owned(),
            max_audit_rowid: audits.to_owned(),
        })
    }

    #[cfg(feature = "d1-test")]
    fn text(&self) -> String {
        format!("{}:{}", self.sessions, self.audits)
    }
}

fn snapshot_value(value: &str) -> PersistenceResult<i64> {
    value
        .parse()
        .map_err(|_| PersistenceError::Corrupt("report snapshot"))
}

#[derive(Default)]
struct ContributionCursor {
    player_id: String,
    session_id: String,
}

struct RosterContributions<'a> {
    repository: &'a D1Repository,
    snapshot: &'a ReportSnapshot,
    game: Option<&'a GameId>,
}

impl BatchSource for RosterContributions<'_> {
    type Cursor = ContributionCursor;
    type Row = ContributionRow;
    async fn next(&self, cursor: &ContributionCursor) -> PersistenceResult<Vec<ContributionRow>> {
        match self.game {
            Some(game) => {
                self.repository
                    .game_contribution_batch(game, self.snapshot, cursor)
                    .await
            }
            None => {
                self.repository
                    .overall_contribution_batch(self.snapshot, cursor)
                    .await
            }
        }
    }
}

struct PlayerContributions<'a> {
    repository: &'a D1Repository,
    snapshot: &'a ReportSnapshot,
    player: &'a PlayerId,
}

impl BatchSource for PlayerContributions<'_> {
    type Cursor = String;
    type Row = AmountRow;
    async fn next(&self, cursor: &String) -> PersistenceResult<Vec<AmountRow>> {
        self.repository
            .player_contribution_batch(self.player, self.snapshot, cursor)
            .await
    }
}

#[derive(Deserialize)]
struct NameRow {
    display_name: String,
}

#[derive(Deserialize)]
struct SessionSummaryRow {
    session_id: String,
    game_name: String,
    occurred_on: String,
}

impl SessionSummaryRow {
    fn decode(self) -> PersistenceResult<SessionSummary> {
        Ok(SessionSummary {
            session_id: SessionId::new(&self.session_id)?,
            game_name: DisplayName::new(&self.game_name)?,
            occurred_on: OccurredOn::parse(&self.occurred_on)?,
        })
    }
}

#[derive(Deserialize)]
struct PlayerSessionRow {
    session_id: String,
    game_name: String,
    occurred_on: String,
    amount_cents: String,
}

impl PlayerSessionRow {
    fn decode(self) -> PersistenceResult<PlayerSessionSummary> {
        Ok(PlayerSessionSummary {
            session: SessionSummaryRow {
                session_id: self.session_id,
                game_name: self.game_name,
                occurred_on: self.occurred_on,
            }
            .decode()?,
            amount: decode_cents(&self.amount_cents)?,
        })
    }
}

#[derive(Deserialize)]
struct DetailEntryRow {
    kind: String,
    identity_id: String,
    display_name: Option<String>,
    amount_cents: String,
}

#[derive(Deserialize)]
struct AuditRow {
    revision: String,
    kind: String,
    actor_id: String,
    reason: String,
    created_at: String,
}

#[derive(Deserialize)]
struct AuditStateRow {
    kind: String,
}

impl AuditRow {
    fn decode(self) -> PersistenceResult<SessionAudit> {
        let kind = match self.kind.as_str() {
            "correction" => AuditKind::Correction,
            "void" => AuditKind::Void,
            _ => return Err(PersistenceError::Corrupt("audit kind")),
        };
        Ok(SessionAudit {
            revision: self
                .revision
                .parse()
                .map_err(|_| PersistenceError::Corrupt("audit revision"))?,
            kind,
            actor: OwnerId::new(
                self.actor_id
                    .parse()
                    .map_err(|_| PersistenceError::Corrupt("audit actor"))?,
            )
            .map_err(corrupt)?,
            reason: AuditReason::new(&self.reason).map_err(corrupt)?,
            created_at: CreatedAt::parse(&self.created_at)
                .map_err(PersistenceError::from)
                .map_err(corrupt)?,
        })
    }
}

impl DetailEntryRow {
    fn decode(self, session: &SessionId) -> PersistenceResult<SessionDetailEntry> {
        let participant = match self.kind.as_str() {
            "roster" => ReportParticipantIdentity::Roster(PlayerId::new(&self.identity_id)?),
            "guest" => {
                ReportParticipantIdentity::Guest(GuestId::new(session.clone(), &self.identity_id)?)
            }
            _ => return Err(PersistenceError::Corrupt("report participant kind")),
        };
        let name = self
            .display_name
            .ok_or(PersistenceError::Corrupt("report participant name"))?;
        Ok(SessionDetailEntry {
            participant,
            display_name: DisplayName::new(&name)?,
            amount: decode_cents(&self.amount_cents)?,
        })
    }
}

impl ReportingRepository for D1Repository {
    async fn leaderboard(
        &self,
        request: ReportPageRequest,
    ) -> PersistenceResult<ReportPage<LeaderboardEntry>> {
        let (roster, snapshot) = self.leaderboard_snapshot().await?;
        let totals = self.overall_totals(&roster, &snapshot).await?;
        leaderboard_page(roster, totals, request)
    }

    async fn game_leaderboard(
        &self,
        game: &GameId,
        request: ReportPageRequest,
    ) -> PersistenceResult<Option<GameLeaderboard>> {
        let statements = self.game_report_statements(game)?;
        let mut results = self.batch(statements).await?.into_iter();
        let Some(game_name) = report_game_name(&mut results)? else {
            return Ok(None);
        };
        let roster = next_rows::<RosterRow>(&mut results, "game report roster query")?;
        let snapshot = next_snapshot(&mut results)?;
        let totals = self.game_totals(game, &roster, &snapshot).await?;
        Ok(Some(GameLeaderboard {
            game_name,
            page: leaderboard_page(roster, totals, request)?,
        }))
    }

    async fn player_report(
        &self,
        player: &PlayerId,
        request: ReportPageRequest,
    ) -> PersistenceResult<Option<PlayerReport>> {
        // D1 captures the player header, read boundary, and history in one snapshot.
        let statements = self.player_report_statements(player, request)?;
        let results = self.batch(statements).await?;
        let Some(head) = decode_player_report_head(results, request)? else {
            return Ok(None);
        };
        self.finish_player_report(player, head).await.map(Some)
    }

    async fn session_history(
        &self,
        request: ReportPageRequest,
    ) -> PersistenceResult<ReportPage<SessionSummary>> {
        let (limit, offset) = page_values(request)?;
        let rows = self
            .rows::<SessionSummaryRow>(SESSION_HISTORY, &[Text(&limit), Text(&offset)])
            .await?;
        Ok(ReportPage::from_extra(decode(rows)?, request))
    }

    async fn session_detail(
        &self,
        session: &SessionId,
        audit_page: AuditPageRequest,
    ) -> PersistenceResult<Option<SessionDetail>> {
        let statements = self.session_detail_statements(session, audit_page)?;
        decode_session_detail(self.batch(statements).await?, session, audit_page)
    }
}

impl D1Repository {
    async fn leaderboard_snapshot(&self) -> PersistenceResult<(Vec<RosterRow>, ReportSnapshot)> {
        let statements = vec![
            self.statement(REPORT_ROSTER, &[])?,
            self.statement(REPORT_SNAPSHOT, &[])?,
        ];
        let mut results = self.batch(statements).await?.into_iter();
        let roster = next_rows(&mut results, "report roster query")?;
        Ok((roster, next_snapshot(&mut results)?))
    }

    fn game_report_statements(
        &self,
        game: &GameId,
    ) -> PersistenceResult<Vec<worker::D1PreparedStatement>> {
        Ok(vec![
            self.statement(GAME, &[Text(game.as_str())])?,
            self.statement(REPORT_ROSTER, &[])?,
            self.statement(REPORT_SNAPSHOT, &[])?,
        ])
    }

    fn player_report_statements(
        &self,
        player: &PlayerId,
        request: ReportPageRequest,
    ) -> PersistenceResult<Vec<worker::D1PreparedStatement>> {
        let (limit, offset) = page_values(request)?;
        Ok(vec![
            self.statement(PLAYER, &[Text(player.as_str())])?,
            self.statement(REPORT_SNAPSHOT, &[])?,
            self.statement(
                PLAYER_SESSIONS,
                &[Text(player.as_str()), Text(&limit), Text(&offset)],
            )?,
        ])
    }

    async fn overall_totals(
        &self,
        roster: &[RosterRow],
        snapshot: &ReportSnapshot,
    ) -> PersistenceResult<BTreeMap<PlayerId, ReportTotal>> {
        let source = RosterContributions {
            repository: self,
            snapshot,
            game: None,
        };
        scan(&source, empty_totals(roster)?, add_contribution_batch).await
    }

    async fn game_totals(
        &self,
        game: &GameId,
        roster: &[RosterRow],
        snapshot: &ReportSnapshot,
    ) -> PersistenceResult<BTreeMap<PlayerId, ReportTotal>> {
        let source = RosterContributions {
            repository: self,
            snapshot,
            game: Some(game),
        };
        scan(&source, empty_totals(roster)?, add_contribution_batch).await
    }

    async fn player_total(
        &self,
        player: &PlayerId,
        snapshot: &ReportSnapshot,
    ) -> PersistenceResult<ReportTotal> {
        let source = PlayerContributions {
            repository: self,
            snapshot,
            player,
        };
        scan(&source, ReportTotal::default(), add_amount_batch).await
    }

    async fn finish_player_report(
        &self,
        player: &PlayerId,
        head: PlayerReportHead,
    ) -> PersistenceResult<PlayerReport> {
        Ok(PlayerReport {
            player_id: head.player_id,
            player_name: head.player_name,
            total: self.player_total(player, &head.snapshot).await?,
            sessions: head.sessions,
        })
    }

    #[cfg(feature = "d1-test")]
    pub(crate) async fn report_snapshot_for_test(&self) -> PersistenceResult<String> {
        let rows = self.rows::<SnapshotRow>(REPORT_SNAPSHOT, &[]).await?;
        let row = rows
            .into_iter()
            .next()
            .ok_or(PersistenceError::Corrupt("report snapshot"))?;
        Ok(ReportSnapshot::decode(row)?.text())
    }

    #[cfg(feature = "d1-test")]
    pub(crate) async fn player_report_at_snapshot_for_test(
        &self,
        player: &PlayerId,
        request: ReportPageRequest,
        snapshot: &str,
    ) -> PersistenceResult<Option<PlayerReport>> {
        let snapshot = ReportSnapshot::from_text(snapshot)?;
        let head = self
            .player_report_head_at_snapshot(player, request, snapshot)
            .await?;
        match head {
            Some(head) => self.finish_player_report(player, head).await.map(Some),
            None => Ok(None),
        }
    }

    #[cfg(feature = "d1-test")]
    pub(crate) async fn leaderboard_at_snapshot_for_test(
        &self,
        game: Option<&GameId>,
        request: ReportPageRequest,
        snapshot: &str,
    ) -> PersistenceResult<ReportPage<LeaderboardEntry>> {
        let snapshot = ReportSnapshot::from_text(snapshot)?;
        let roster = self.rows::<RosterRow>(REPORT_ROSTER, &[]).await?;
        let totals = match game {
            Some(game) => self.game_totals(game, &roster, &snapshot).await?,
            None => self.overall_totals(&roster, &snapshot).await?,
        };
        leaderboard_page(roster, totals, request)
    }

    #[cfg(feature = "d1-test")]
    async fn player_report_head_at_snapshot(
        &self,
        player: &PlayerId,
        request: ReportPageRequest,
        snapshot: ReportSnapshot,
    ) -> PersistenceResult<Option<PlayerReportHead>> {
        let statements = self.player_report_at_snapshot_statements(player, request, &snapshot)?;
        decode_player_report_at_snapshot(self.batch(statements).await?, request, snapshot)
    }

    #[cfg(feature = "d1-test")]
    fn player_report_at_snapshot_statements(
        &self,
        player: &PlayerId,
        request: ReportPageRequest,
        snapshot: &ReportSnapshot,
    ) -> PersistenceResult<Vec<worker::D1PreparedStatement>> {
        let (limit, offset) = page_values(request)?;
        Ok(vec![
            self.statement(PLAYER, &[Text(player.as_str())])?,
            self.statement(
                PLAYER_SESSIONS_AT_SNAPSHOT,
                &[
                    Text(player.as_str()),
                    Text(&snapshot.sessions),
                    Text(&snapshot.audits),
                    Text(&limit),
                    Text(&offset),
                ],
            )?,
        ])
    }

    fn session_detail_statements(
        &self,
        session: &SessionId,
        request: AuditPageRequest,
    ) -> PersistenceResult<Vec<worker::D1PreparedStatement>> {
        let limit = request
            .size()
            .checked_add(1)
            .ok_or(PersistenceError::InvalidPage)?
            .to_string();
        let after = request.after_revision().to_string();
        Ok(vec![
            self.statement(SESSION_HEADER, &[Text(session.as_str())])?,
            self.statement(SESSION_DETAIL_ENTRIES, &[Text(session.as_str())])?,
            self.statement(
                SESSION_AUDITS,
                &[Text(session.as_str()), Text(&after), Text(&limit)],
            )?,
            self.statement(SESSION_AUDIT_STATE, &[Text(session.as_str())])?,
        ])
    }

    async fn overall_contribution_batch(
        &self,
        snapshot: &ReportSnapshot,
        cursor: &ContributionCursor,
    ) -> PersistenceResult<Vec<ContributionRow>> {
        let limit = REPORT_CONTRIBUTION_BATCH_SIZE.to_string();
        self.rows(
            OVERALL_CONTRIBUTIONS,
            &[
                Text(&snapshot.sessions),
                Text(&snapshot.audits),
                Text(&cursor.player_id),
                Text(&cursor.session_id),
                Text(&limit),
            ],
        )
        .await
    }

    async fn game_contribution_batch(
        &self,
        game: &GameId,
        snapshot: &ReportSnapshot,
        cursor: &ContributionCursor,
    ) -> PersistenceResult<Vec<ContributionRow>> {
        let limit = REPORT_CONTRIBUTION_BATCH_SIZE.to_string();
        self.rows(
            GAME_CONTRIBUTIONS,
            &[
                Text(game.as_str()),
                Text(&snapshot.sessions),
                Text(&snapshot.audits),
                Text(&cursor.player_id),
                Text(&cursor.session_id),
                Text(&limit),
            ],
        )
        .await
    }

    async fn player_contribution_batch(
        &self,
        player: &PlayerId,
        snapshot: &ReportSnapshot,
        cursor: &str,
    ) -> PersistenceResult<Vec<AmountRow>> {
        let limit = REPORT_CONTRIBUTION_BATCH_SIZE.to_string();
        self.rows(
            PLAYER_CONTRIBUTIONS,
            &[
                Text(player.as_str()),
                Text(&snapshot.sessions),
                Text(&snapshot.audits),
                Text(cursor),
                Text(&limit),
            ],
        )
        .await
    }
}

fn decode_session_detail(
    results: Vec<D1Result>,
    session: &SessionId,
    request: AuditPageRequest,
) -> PersistenceResult<Option<SessionDetail>> {
    let mut results = results.into_iter();
    let headers = next_rows::<SessionSummaryRow>(&mut results, "session detail header query")?;
    let Some(header) = headers.into_iter().next() else {
        return Ok(None);
    };
    let entries = decode_detail_entries(&mut results, session)?;
    validate_detail(&entries)?;
    Ok(Some(SessionDetail {
        session: header.decode().map_err(corrupt)?,
        entries,
        audits: decode_detail_audits(&mut results, request)?,
        state: decode_detail_state(&mut results)?,
    }))
}

fn decode_detail_entries(
    results: &mut impl Iterator<Item = D1Result>,
    session: &SessionId,
) -> PersistenceResult<Vec<SessionDetailEntry>> {
    next_rows::<DetailEntryRow>(results, "session detail entries query")?
        .into_iter()
        .map(|row| row.decode(session).map_err(corrupt))
        .collect()
}

fn decode_detail_audits(
    results: &mut impl Iterator<Item = D1Result>,
    request: AuditPageRequest,
) -> PersistenceResult<SessionAuditPage> {
    let audits = next_rows::<AuditRow>(results, "session detail audits query")?
        .into_iter()
        .map(|row| row.decode().map_err(corrupt))
        .collect::<PersistenceResult<Vec<_>>>()?;
    Ok(SessionAuditPage::from_extra(audits, request))
}

fn decode_detail_state(
    results: &mut impl Iterator<Item = D1Result>,
) -> PersistenceResult<AuditedSessionState> {
    let kind = next_rows::<AuditStateRow>(results, "session detail state query")?
        .into_iter()
        .next()
        .map(|row| row.kind);
    audit_state(kind.as_deref())
}

fn report_game_name(
    results: &mut impl Iterator<Item = D1Result>,
) -> PersistenceResult<Option<DisplayName>> {
    let rows = next_rows::<NameRow>(results, "game report header query")?;
    rows.into_iter()
        .next()
        .map(|row| decode_name(&row.display_name))
        .transpose()
}

struct PlayerReportHead {
    player_id: PlayerId,
    player_name: DisplayName,
    snapshot: ReportSnapshot,
    sessions: ReportPage<PlayerSessionSummary>,
}

fn decode_player_report_head(
    results: Vec<D1Result>,
    request: ReportPageRequest,
) -> PersistenceResult<Option<PlayerReportHead>> {
    let mut results = results.into_iter();
    let header = next_rows::<RosterRow>(&mut results, "player report header query")?;
    let snapshot = next_snapshot(&mut results)?;
    let sessions = next_rows::<PlayerSessionRow>(&mut results, "player sessions query")?;
    let Some(header) = header.into_iter().next() else {
        return Ok(None);
    };
    let (player_id, player_name) = header.decode().map_err(corrupt)?;
    Ok(Some(PlayerReportHead {
        player_id,
        player_name,
        snapshot,
        sessions: ReportPage::from_extra(decode(sessions)?, request),
    }))
}

#[cfg(feature = "d1-test")]
fn decode_player_report_at_snapshot(
    results: Vec<D1Result>,
    request: ReportPageRequest,
    snapshot: ReportSnapshot,
) -> PersistenceResult<Option<PlayerReportHead>> {
    let mut results = results.into_iter();
    let header = next_rows::<RosterRow>(&mut results, "player report header query")?;
    let sessions = next_rows::<PlayerSessionRow>(&mut results, "player sessions query")?;
    let Some(header) = header.into_iter().next() else {
        return Ok(None);
    };
    let (player_id, player_name) = header.decode().map_err(corrupt)?;
    Ok(Some(PlayerReportHead {
        player_id,
        player_name,
        snapshot,
        sessions: ReportPage::from_extra(decode(sessions)?, request),
    }))
}

fn leaderboard_page(
    roster: Vec<RosterRow>,
    totals: BTreeMap<PlayerId, ReportTotal>,
    request: ReportPageRequest,
) -> PersistenceResult<ReportPage<LeaderboardEntry>> {
    let roster = decode_roster(roster)?;
    let mut entries = leaderboard_entries(roster, totals)?;
    entries.sort_by(compare_leaderboard_entries);
    paginate(entries, request)
}

fn decode_roster(rows: Vec<RosterRow>) -> PersistenceResult<Vec<(PlayerId, DisplayName)>> {
    rows.into_iter()
        .map(|row| row.decode().map_err(corrupt))
        .collect()
}

fn leaderboard_entries(
    roster: Vec<(PlayerId, DisplayName)>,
    mut totals: BTreeMap<PlayerId, ReportTotal>,
) -> PersistenceResult<Vec<LeaderboardEntry>> {
    roster
        .into_iter()
        .map(|player| leaderboard_entry(player, &mut totals))
        .collect()
}

fn leaderboard_entry(
    (player_id, player_name): (PlayerId, DisplayName),
    totals: &mut BTreeMap<PlayerId, ReportTotal>,
) -> PersistenceResult<LeaderboardEntry> {
    let total = totals
        .remove(&player_id)
        .ok_or(PersistenceError::Corrupt("report roster total"))?;
    Ok(LeaderboardEntry {
        player_id,
        player_name,
        total,
    })
}

fn compare_leaderboard_entries(
    left: &LeaderboardEntry,
    right: &LeaderboardEntry,
) -> std::cmp::Ordering {
    right
        .total
        .cmp(&left.total)
        .then_with(|| left.player_name.as_str().cmp(right.player_name.as_str()))
        .then_with(|| left.player_id.cmp(&right.player_id))
}

fn paginate<T>(items: Vec<T>, request: ReportPageRequest) -> PersistenceResult<ReportPage<T>> {
    let count = request
        .size()
        .checked_add(1)
        .ok_or(PersistenceError::InvalidPage)?;
    let items = items
        .into_iter()
        .skip(request.offset()?)
        .take(count)
        .collect();
    Ok(ReportPage::from_extra(items, request))
}

fn empty_totals(rows: &[RosterRow]) -> PersistenceResult<BTreeMap<PlayerId, ReportTotal>> {
    rows.iter()
        .map(|row| Ok((PlayerId::new(&row.player_id)?, ReportTotal::default())))
        .collect()
}

fn add_contribution_batch(
    totals: &mut BTreeMap<PlayerId, ReportTotal>,
    cursor: &mut ContributionCursor,
    rows: Vec<ContributionRow>,
) -> PersistenceResult<()> {
    for row in rows {
        let (player, amount) = row.decode().map_err(corrupt)?;
        let total = totals
            .get_mut(&player)
            .ok_or(PersistenceError::Corrupt("report contributor"))?;
        total.add(amount)?;
        cursor.player_id = row.player_id;
        cursor.session_id = row.session_id;
    }
    Ok(())
}

fn add_amount_batch(
    total: &mut ReportTotal,
    cursor: &mut String,
    rows: Vec<AmountRow>,
) -> PersistenceResult<()> {
    for row in rows {
        total.add(decode_cents(&row.amount_cents)?)?;
        *cursor = row.session_id;
    }
    Ok(())
}

fn next_snapshot(
    results: &mut impl Iterator<Item = D1Result>,
) -> PersistenceResult<ReportSnapshot> {
    let rows = next_rows::<SnapshotRow>(results, "report snapshot query")?;
    let row = rows
        .into_iter()
        .next()
        .ok_or(PersistenceError::Corrupt("report snapshot"))?;
    ReportSnapshot::decode(row)
}

fn next_rows<T: DeserializeOwned>(
    results: &mut impl Iterator<Item = D1Result>,
    error: &'static str,
) -> PersistenceResult<Vec<T>> {
    let result = results.next().ok_or(PersistenceError::Corrupt(error))?;
    decode_rows(result)
}

fn decode<T, U>(rows: Vec<T>) -> PersistenceResult<Vec<U>>
where
    T: Decode<U>,
{
    rows.into_iter()
        .map(|row| row.decode_value().map_err(corrupt))
        .collect()
}

trait Decode<T> {
    fn decode_value(self) -> PersistenceResult<T>;
}

impl Decode<SessionSummary> for SessionSummaryRow {
    fn decode_value(self) -> PersistenceResult<SessionSummary> {
        self.decode()
    }
}

impl Decode<PlayerSessionSummary> for PlayerSessionRow {
    fn decode_value(self) -> PersistenceResult<PlayerSessionSummary> {
        self.decode()
    }
}

fn page_values(request: ReportPageRequest) -> PersistenceResult<(String, String)> {
    let limit = request
        .size()
        .checked_add(1)
        .ok_or(PersistenceError::InvalidPage)?;
    Ok((limit.to_string(), request.offset()?.to_string()))
}

fn decode_cents(value: &str) -> PersistenceResult<Cents> {
    value
        .parse()
        .map(Cents::new)
        .map_err(|_| PersistenceError::Corrupt("report integer cents"))
}

fn decode_name(value: &str) -> PersistenceResult<DisplayName> {
    DisplayName::new(value)
        .map_err(PersistenceError::from)
        .map_err(corrupt)
}

fn validate_detail(entries: &[SessionDetailEntry]) -> PersistenceResult<()> {
    if entries.len() < 2 {
        return Err(PersistenceError::Corrupt("report session entries"));
    }
    let total = Cents::checked_sum(entries.iter().map(|entry| entry.amount))
        .map_err(|_| PersistenceError::Corrupt("report session total"))?;
    if total != Cents::ZERO {
        return Err(PersistenceError::Corrupt("report session total"));
    }
    Ok(())
}

fn audit_state(kind: Option<&str>) -> PersistenceResult<AuditedSessionState> {
    match kind {
        None => Ok(AuditedSessionState::Original),
        Some("correction") => Ok(AuditedSessionState::Corrected),
        Some("void") => Ok(AuditedSessionState::Voided),
        Some(_) => Err(PersistenceError::Corrupt("audit state")),
    }
}
