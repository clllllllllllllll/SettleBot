use std::collections::BTreeSet;

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
const OVERALL_CONTRIBUTIONS: &str = "
    SELECT roster_player_id AS player_id, CAST(amount_cents AS TEXT) AS amount_cents
    FROM session_entries WHERE roster_player_id IS NOT NULL
    ORDER BY roster_player_id, session_id";
const GAME: &str = "SELECT display_name FROM games WHERE id = ?";
const GAME_CONTRIBUTIONS: &str = "
    SELECT entries.roster_player_id AS player_id,
           CAST(entries.amount_cents AS TEXT) AS amount_cents
    FROM session_entries AS entries JOIN sessions ON sessions.id = entries.session_id
    WHERE entries.roster_player_id IS NOT NULL AND sessions.game_id = ?
    ORDER BY entries.roster_player_id, entries.session_id";
const PLAYER: &str = "
    SELECT id AS player_id, display_name AS player_name FROM players WHERE id = ?";
const PLAYER_CONTRIBUTIONS: &str = "
    SELECT CAST(amount_cents AS TEXT) AS amount_cents
    FROM session_entries WHERE roster_player_id = ? ORDER BY session_id";
const PLAYER_SESSIONS: &str = "
    SELECT sessions.id AS session_id, games.display_name AS game_name,
           sessions.occurred_on, CAST(entries.amount_cents AS TEXT) AS amount_cents
    FROM session_entries AS entries
    JOIN sessions ON sessions.id = entries.session_id
    JOIN games ON games.id = sessions.game_id
    WHERE entries.roster_player_id = ?1
    ORDER BY sessions.occurred_on DESC, sessions.id DESC
    LIMIT CAST(?2 AS INTEGER) OFFSET CAST(?3 AS INTEGER)";
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
    amount_cents: String,
}

impl ContributionRow {
    fn decode(self) -> PersistenceResult<(PlayerId, Cents)> {
        Ok((
            PlayerId::new(&self.player_id)?,
            decode_cents(&self.amount_cents)?,
        ))
    }
}

#[derive(Deserialize)]
struct AmountRow {
    amount_cents: String,
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
        let statements = vec![
            self.statement(REPORT_ROSTER, &[])?,
            self.statement(OVERALL_CONTRIBUTIONS, &[])?,
        ];
        let mut results = self.batch(statements).await?.into_iter();
        let roster = next_rows::<RosterRow>(&mut results, "report roster query")?;
        let contributions = next_rows(&mut results, "report contribution query")?;
        leaderboard_page(roster, contributions, request)
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
        let contributions = next_rows(&mut results, "game contribution query")?;
        Ok(Some(GameLeaderboard {
            game_name,
            page: leaderboard_page(roster, contributions, request)?,
        }))
    }

    async fn player_report(
        &self,
        player: &PlayerId,
        request: ReportPageRequest,
    ) -> PersistenceResult<Option<PlayerReport>> {
        // D1 executes the header, contributions, and history in one transactional batch.
        let statements = self.player_report_statements(player, request)?;
        let results = self.batch(statements).await?;
        decode_player_report(results, request)
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
    fn game_report_statements(
        &self,
        game: &GameId,
    ) -> PersistenceResult<Vec<worker::D1PreparedStatement>> {
        Ok(vec![
            self.statement(GAME, &[Text(game.as_str())])?,
            self.statement(REPORT_ROSTER, &[])?,
            self.statement(GAME_CONTRIBUTIONS, &[Text(game.as_str())])?,
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
            self.statement(PLAYER_CONTRIBUTIONS, &[Text(player.as_str())])?,
            self.statement(
                PLAYER_SESSIONS,
                &[Text(player.as_str()), Text(&limit), Text(&offset)],
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

fn decode_player_report(
    results: Vec<D1Result>,
    request: ReportPageRequest,
) -> PersistenceResult<Option<PlayerReport>> {
    let mut results = results.into_iter();
    let header = next_rows::<RosterRow>(&mut results, "player report header query")?;
    let amounts = next_rows::<AmountRow>(&mut results, "player contribution query")?;
    let sessions = next_rows::<PlayerSessionRow>(&mut results, "player sessions query")?;
    let Some(header) = header.into_iter().next() else {
        return Ok(None);
    };
    let (player_id, player_name) = header.decode().map_err(corrupt)?;
    Ok(Some(PlayerReport {
        player_id,
        player_name,
        total: aggregate_amounts(amounts)?,
        sessions: ReportPage::from_extra(decode(sessions)?, request),
    }))
}

fn leaderboard_page(
    roster: Vec<RosterRow>,
    contributions: Vec<ContributionRow>,
    request: ReportPageRequest,
) -> PersistenceResult<ReportPage<LeaderboardEntry>> {
    let roster = decode_roster(roster)?;
    let contributions = decode_contributions(contributions)?;
    validate_contributors(&roster, &contributions)?;
    let mut entries = leaderboard_entries(roster, &contributions)?;
    entries.sort_by(compare_leaderboard_entries);
    paginate(entries, request)
}

fn decode_roster(rows: Vec<RosterRow>) -> PersistenceResult<Vec<(PlayerId, DisplayName)>> {
    rows.into_iter()
        .map(|row| row.decode().map_err(corrupt))
        .collect()
}

fn decode_contributions(rows: Vec<ContributionRow>) -> PersistenceResult<Vec<(PlayerId, Cents)>> {
    rows.into_iter()
        .map(|row| row.decode().map_err(corrupt))
        .collect()
}

fn validate_contributors(
    roster: &[(PlayerId, DisplayName)],
    contributions: &[(PlayerId, Cents)],
) -> PersistenceResult<()> {
    let ids: BTreeSet<_> = roster.iter().map(|(id, _)| id).collect();
    if contributions.iter().any(|(id, _)| !ids.contains(id)) {
        return Err(PersistenceError::Corrupt("report contributor"));
    }
    Ok(())
}

fn leaderboard_entries(
    roster: Vec<(PlayerId, DisplayName)>,
    contributions: &[(PlayerId, Cents)],
) -> PersistenceResult<Vec<LeaderboardEntry>> {
    roster
        .into_iter()
        .map(|player| leaderboard_entry(player, contributions))
        .collect()
}

fn leaderboard_entry(
    (player_id, player_name): (PlayerId, DisplayName),
    contributions: &[(PlayerId, Cents)],
) -> PersistenceResult<LeaderboardEntry> {
    let amounts = contributions
        .iter()
        .filter(|(id, _)| id == &player_id)
        .map(|(_, amount)| *amount);
    let total = checked_report_total(amounts)?;
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

fn aggregate_amounts(rows: Vec<AmountRow>) -> PersistenceResult<Cents> {
    let amounts = rows
        .into_iter()
        .map(|row| decode_cents(&row.amount_cents))
        .collect::<PersistenceResult<Vec<_>>>()?;
    checked_report_total(amounts)
}

fn checked_report_total(amounts: impl IntoIterator<Item = Cents>) -> PersistenceResult<Cents> {
    Cents::checked_sum(amounts).map_err(|_| PersistenceError::Corrupt("report aggregate overflow"))
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
