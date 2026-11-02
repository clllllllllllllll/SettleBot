use std::collections::BTreeMap;

use serde::{de::DeserializeOwned, Deserialize};
use worker::{D1Result, D1Type::Text};

use crate::domain::{Cents, DisplayName, GameId, GuestId, OccurredOn, PlayerId, SessionId};
use crate::persistence::{
    GameLeaderboard, LeaderboardEntry, PersistenceError, PersistenceResult, PlayerReport,
    PlayerSessionSummary, ReportPage, ReportPageRequest, ReportParticipantIdentity,
    ReportingRepository, SessionDetail, SessionDetailEntry, SessionSummary,
};

use super::{decode_rows, records::corrupt, D1Repository};

const REPORT_ROSTER: &str = "
    SELECT id AS player_id, display_name AS player_name FROM players ORDER BY id";
const REPORT_SNAPSHOT: &str = "
    SELECT CAST(COALESCE(MAX(rowid), 0) AS TEXT) AS max_session_rowid FROM sessions";
const OVERALL_CONTRIBUTIONS: &str = "
    SELECT entries.roster_player_id AS player_id, entries.session_id,
           CAST(entries.amount_cents AS TEXT) AS amount_cents
    FROM session_entries AS entries JOIN sessions ON sessions.id = entries.session_id
    WHERE entries.roster_player_id IS NOT NULL
      AND sessions.rowid <= CAST(?1 AS INTEGER)
      AND (entries.roster_player_id > ?2
           OR (entries.roster_player_id = ?2 AND entries.session_id > ?3))
    ORDER BY entries.roster_player_id, entries.session_id
    LIMIT CAST(?4 AS INTEGER)";
const GAME: &str = "SELECT display_name FROM games WHERE id = ?";
const GAME_CONTRIBUTIONS: &str = "
    SELECT entries.roster_player_id AS player_id, entries.session_id,
           CAST(entries.amount_cents AS TEXT) AS amount_cents
    FROM session_entries AS entries JOIN sessions ON sessions.id = entries.session_id
    WHERE entries.roster_player_id IS NOT NULL AND sessions.game_id = ?1
      AND sessions.rowid <= CAST(?2 AS INTEGER)
      AND (entries.roster_player_id > ?3
           OR (entries.roster_player_id = ?3 AND entries.session_id > ?4))
    ORDER BY entries.roster_player_id, entries.session_id
    LIMIT CAST(?5 AS INTEGER)";
const PLAYER: &str = "
    SELECT id AS player_id, display_name AS player_name FROM players WHERE id = ?";
const PLAYER_CONTRIBUTIONS: &str = "
    SELECT entries.session_id, CAST(entries.amount_cents AS TEXT) AS amount_cents
    FROM session_entries AS entries JOIN sessions ON sessions.id = entries.session_id
    WHERE entries.roster_player_id = ?1
      AND sessions.rowid <= CAST(?2 AS INTEGER) AND entries.session_id > ?3
    ORDER BY entries.session_id LIMIT CAST(?4 AS INTEGER)";
const PLAYER_SESSIONS: &str = "
    SELECT sessions.id AS session_id, games.display_name AS game_name,
           sessions.occurred_on, CAST(entries.amount_cents AS TEXT) AS amount_cents
    FROM session_entries AS entries
    JOIN sessions ON sessions.id = entries.session_id
    JOIN games ON games.id = sessions.game_id
    WHERE entries.roster_player_id = ?1
      AND sessions.rowid <= (SELECT COALESCE(MAX(rowid), 0) FROM sessions)
    ORDER BY sessions.occurred_on DESC, sessions.id DESC
    LIMIT CAST(?2 AS INTEGER) OFFSET CAST(?3 AS INTEGER)";
#[cfg(feature = "d1-test")]
const PLAYER_SESSIONS_AT_SNAPSHOT: &str = "
    SELECT sessions.id AS session_id, games.display_name AS game_name,
           sessions.occurred_on, CAST(entries.amount_cents AS TEXT) AS amount_cents
    FROM session_entries AS entries
    JOIN sessions ON sessions.id = entries.session_id
    JOIN games ON games.id = sessions.game_id
    WHERE entries.roster_player_id = ?1 AND sessions.rowid <= CAST(?2 AS INTEGER)
    ORDER BY sessions.occurred_on DESC, sessions.id DESC
    LIMIT CAST(?3 AS INTEGER) OFFSET CAST(?4 AS INTEGER)";
const SESSION_HISTORY: &str = "
    SELECT sessions.id AS session_id, games.display_name AS game_name, sessions.occurred_on
    FROM sessions JOIN games ON games.id = sessions.game_id
    ORDER BY sessions.occurred_on DESC, sessions.id DESC
    LIMIT CAST(?1 AS INTEGER) OFFSET CAST(?2 AS INTEGER)";
const SESSION_HEADER: &str = "
    SELECT sessions.id AS session_id, games.display_name AS game_name, sessions.occurred_on
    FROM sessions JOIN games ON games.id = sessions.game_id WHERE sessions.id = ?";
const SESSION_DETAIL_ENTRIES: &str = "
    SELECT entries.kind, entries.identity_id,
           CASE WHEN entries.kind = 'roster' THEN players.display_name ELSE entries.guest_name END
               AS display_name,
           CAST(entries.amount_cents AS TEXT) AS amount_cents
    FROM session_entries AS entries
    LEFT JOIN players ON players.id = entries.roster_player_id
    WHERE entries.session_id = ? ORDER BY entries.position";

const REPORT_CONTRIBUTION_BATCH_SIZE: usize = 100;

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
}

#[derive(Clone)]
struct ReportSnapshot(String);

impl ReportSnapshot {
    fn decode(row: SnapshotRow) -> PersistenceResult<Self> {
        let value = row
            .max_session_rowid
            .parse::<i64>()
            .map_err(|_| PersistenceError::Corrupt("report snapshot"))?;
        if value < 0 {
            return Err(PersistenceError::Corrupt("report snapshot"));
        }
        Ok(Self(row.max_session_rowid))
    }

    #[cfg(feature = "d1-test")]
    fn from_text(value: &str) -> PersistenceResult<Self> {
        Self::decode(SnapshotRow {
            max_session_rowid: value.to_owned(),
        })
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Default)]
struct WideTotal(i128);

impl WideTotal {
    fn add(&mut self, amount: Cents) -> PersistenceResult<()> {
        self.0 = self
            .0
            .checked_add(i128::from(amount.value()))
            .ok_or(PersistenceError::Corrupt("report aggregate overflow"))?;
        Ok(())
    }

    fn cents(self) -> PersistenceResult<Cents> {
        i64::try_from(self.0)
            .map(Cents::new)
            .map_err(|_| PersistenceError::Corrupt("report aggregate overflow"))
    }
}

#[derive(Default)]
struct ContributionCursor {
    player_id: String,
    session_id: String,
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
    ) -> PersistenceResult<Option<SessionDetail>> {
        let Some(header) = self.report_session_header(session).await? else {
            return Ok(None);
        };
        let entries = self.report_detail_entries(session).await?;
        validate_detail(&entries)?;
        Ok(Some(SessionDetail {
            session: header,
            entries,
        }))
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
    ) -> PersistenceResult<BTreeMap<PlayerId, WideTotal>> {
        let mut totals = empty_totals(roster)?;
        let mut cursor = ContributionCursor::default();
        loop {
            let rows = self.overall_contribution_batch(snapshot, &cursor).await?;
            if add_contribution_batch(&mut totals, &mut cursor, rows)? {
                return Ok(totals);
            }
        }
    }

    async fn game_totals(
        &self,
        game: &GameId,
        roster: &[RosterRow],
        snapshot: &ReportSnapshot,
    ) -> PersistenceResult<BTreeMap<PlayerId, WideTotal>> {
        let mut totals = empty_totals(roster)?;
        let mut cursor = ContributionCursor::default();
        loop {
            let rows = self
                .game_contribution_batch(game, snapshot, &cursor)
                .await?;
            if add_contribution_batch(&mut totals, &mut cursor, rows)? {
                return Ok(totals);
            }
        }
    }

    async fn player_total(
        &self,
        player: &PlayerId,
        snapshot: &ReportSnapshot,
    ) -> PersistenceResult<Cents> {
        let mut total = WideTotal::default();
        let mut cursor = String::new();
        loop {
            let rows = self
                .player_contribution_batch(player, snapshot, &cursor)
                .await?;
            if add_amount_batch(&mut total, &mut cursor, rows)? {
                return total.cents();
            }
        }
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
        Ok(ReportSnapshot::decode(row)?.0)
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
                    Text(snapshot.as_str()),
                    Text(&limit),
                    Text(&offset),
                ],
            )?,
        ])
    }

    async fn report_session_header(
        &self,
        session: &SessionId,
    ) -> PersistenceResult<Option<SessionSummary>> {
        let row = self
            .rows::<SessionSummaryRow>(SESSION_HEADER, &[Text(session.as_str())])
            .await?
            .into_iter()
            .next();
        row.map(|row| row.decode().map_err(corrupt)).transpose()
    }

    async fn report_detail_entries(
        &self,
        session: &SessionId,
    ) -> PersistenceResult<Vec<SessionDetailEntry>> {
        let rows = self
            .rows::<DetailEntryRow>(SESSION_DETAIL_ENTRIES, &[Text(session.as_str())])
            .await?;
        rows.into_iter()
            .map(|row| row.decode(session).map_err(corrupt))
            .collect()
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
                Text(snapshot.as_str()),
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
                Text(snapshot.as_str()),
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
                Text(snapshot.as_str()),
                Text(cursor),
                Text(&limit),
            ],
        )
        .await
    }
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
    totals: BTreeMap<PlayerId, WideTotal>,
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
    mut totals: BTreeMap<PlayerId, WideTotal>,
) -> PersistenceResult<Vec<LeaderboardEntry>> {
    roster
        .into_iter()
        .map(|player| leaderboard_entry(player, &mut totals))
        .collect()
}

fn leaderboard_entry(
    (player_id, player_name): (PlayerId, DisplayName),
    totals: &mut BTreeMap<PlayerId, WideTotal>,
) -> PersistenceResult<LeaderboardEntry> {
    let total = totals
        .remove(&player_id)
        .ok_or(PersistenceError::Corrupt("report roster total"))?
        .cents()?;
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

fn empty_totals(rows: &[RosterRow]) -> PersistenceResult<BTreeMap<PlayerId, WideTotal>> {
    rows.iter()
        .map(|row| Ok((PlayerId::new(&row.player_id)?, WideTotal::default())))
        .collect()
}

fn add_contribution_batch(
    totals: &mut BTreeMap<PlayerId, WideTotal>,
    cursor: &mut ContributionCursor,
    rows: Vec<ContributionRow>,
) -> PersistenceResult<bool> {
    let complete = rows.len() < REPORT_CONTRIBUTION_BATCH_SIZE;
    for row in rows {
        let (player, amount) = row.decode().map_err(corrupt)?;
        let total = totals
            .get_mut(&player)
            .ok_or(PersistenceError::Corrupt("report contributor"))?;
        total.add(amount)?;
        cursor.player_id = row.player_id;
        cursor.session_id = row.session_id;
    }
    Ok(complete)
}

fn add_amount_batch(
    total: &mut WideTotal,
    cursor: &mut String,
    rows: Vec<AmountRow>,
) -> PersistenceResult<bool> {
    let complete = rows.len() < REPORT_CONTRIBUTION_BATCH_SIZE;
    for row in rows {
        total.add(decode_cents(&row.amount_cents)?)?;
        *cursor = row.session_id;
    }
    Ok(complete)
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
