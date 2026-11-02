use super::{commands::ReportCommand, policy::PageSize, render::Reply, Error, Repository, Result};
use crate::persistence::{
    LeaderboardEntry, PlayerReport, ReportPage, ReportPageRequest, ReportParticipantIdentity,
    SessionDetail, SessionSummary,
};

const REPORT_PAGE_SIZE: usize = 10;

pub(super) async fn reply(
    repository: &impl Repository,
    chat: i64,
    command: ReportCommand,
) -> Result<Reply> {
    match command {
        ReportCommand::Leaderboard { page } => leaderboard(repository, chat, page).await,
        ReportCommand::GameLeaderboard { game, page } => {
            game_leaderboard(repository, chat, game, page).await
        }
        ReportCommand::Player { player, page } => {
            player_report(repository, chat, player, page).await
        }
        ReportCommand::History { page } => history(repository, chat, page).await,
        ReportCommand::Session { session } => details(repository, chat, session).await,
    }
}

async fn leaderboard(repository: &impl Repository, chat: i64, number: u32) -> Result<Reply> {
    let request = request(number)?;
    let page = repository.leaderboard(request).await?;
    let mut text = format!("Overall leaderboard — page {number}");
    append_leaderboard(&mut text, &page, request)?;
    append_navigation(&mut text, &page, "/leaderboard");
    Ok(Reply::text(chat, text))
}

async fn game_leaderboard(
    repository: &impl Repository,
    chat: i64,
    game: crate::domain::GameId,
    number: u32,
) -> Result<Reply> {
    let request = request(number)?;
    let report = repository
        .game_leaderboard(&game, request)
        .await?
        .ok_or(Error::Invalid)?;
    let mut text = format!("{} leaderboard — page {number}", report.game_name.as_str());
    append_leaderboard(&mut text, &report.page, request)?;
    append_navigation(
        &mut text,
        &report.page,
        &format!("/gameboard {}", game.as_str()),
    );
    Ok(Reply::text(chat, text))
}

async fn player_report(
    repository: &impl Repository,
    chat: i64,
    player: crate::domain::PlayerId,
    number: u32,
) -> Result<Reply> {
    let request = request(number)?;
    let report = repository
        .player_report(&player, request)
        .await?
        .ok_or(Error::Invalid)?;
    Ok(Reply::text(chat, render_player(&report, number)))
}

async fn history(repository: &impl Repository, chat: i64, number: u32) -> Result<Reply> {
    let page = repository.session_history(request(number)?).await?;
    let mut text = format!("Session history — page {number}");
    for session in page.items() {
        append_session(&mut text, session);
    }
    if page.items().is_empty() {
        text += "\nNo saved sessions.";
    }
    append_navigation(&mut text, &page, "/history");
    Ok(Reply::text(chat, text))
}

async fn details(
    repository: &impl Repository,
    chat: i64,
    session: crate::domain::SessionId,
) -> Result<Reply> {
    let detail = repository
        .session_detail(&session)
        .await?
        .ok_or(Error::Invalid)?;
    Ok(Reply::text(chat, render_detail(&detail)))
}

fn request(number: u32) -> Result<ReportPageRequest> {
    let size = PageSize::new(REPORT_PAGE_SIZE)?.value();
    ReportPageRequest::new(number, size).map_err(Into::into)
}

fn append_leaderboard(
    text: &mut String,
    page: &ReportPage<LeaderboardEntry>,
    request: ReportPageRequest,
) -> Result<()> {
    let start = request.offset().map_err(Error::from)?;
    for (index, entry) in page.items().iter().enumerate() {
        *text += &format!(
            "\n{}. {} [{}]: {}",
            start + index + 1,
            entry.player_name.as_str(),
            entry.player_id.as_str(),
            entry.total
        );
    }
    if page.items().is_empty() {
        *text += "\nNo roster players.";
    }
    Ok(())
}

fn render_player(report: &PlayerReport, number: u32) -> String {
    let mut text = format!(
        "Player: {} [{}]\nOverall: {}\nSessions — page {number}",
        report.player_name.as_str(),
        report.player_id.as_str(),
        report.total
    );
    append_player_sessions(&mut text, report);
    append_navigation(
        &mut text,
        &report.sessions,
        &format!("/stats {}", report.player_id.as_str()),
    );
    text
}

fn append_player_sessions(text: &mut String, report: &PlayerReport) {
    for row in report.sessions.items() {
        *text += &format!(
            "\n{} — {} [{}]: {}",
            row.session.occurred_on,
            row.session.game_name.as_str(),
            row.session.session_id.as_str(),
            row.amount
        );
    }
    if report.sessions.items().is_empty() {
        *text += "\nNo sessions for this player.";
    }
}

fn append_session(text: &mut String, session: &SessionSummary) {
    *text += &format!(
        "\n{} — {} [{}] — /details {}",
        session.occurred_on,
        session.game_name.as_str(),
        session.session_id.as_str(),
        session.session_id.as_str()
    );
}

fn render_detail(detail: &SessionDetail) -> String {
    let session = &detail.session;
    let mut text = format!(
        "Session {}\nGame: {}\nPlayed: {} (Asia/Singapore)",
        session.session_id.as_str(),
        session.game_name.as_str(),
        session.occurred_on
    );
    append_detail_entries(&mut text, detail);
    text
}

fn append_detail_entries(text: &mut String, detail: &SessionDetail) {
    for entry in &detail.entries {
        let (kind, id) = match &entry.participant {
            ReportParticipantIdentity::Roster(id) => ("roster", id.as_str()),
            ReportParticipantIdentity::Guest(id) => ("one-time guest", id.as_str()),
        };
        *text += &format!(
            "\n{} [{}; {kind}]: {}",
            entry.display_name.as_str(),
            id,
            entry.amount
        );
    }
}

fn append_navigation<T>(text: &mut String, page: &ReportPage<T>, command: &str) {
    if page.number() > 1 {
        *text += &format!("\nPrevious: {command} {}", page.number() - 1);
    }
    if page.has_more() {
        *text += &format!("\nNext: {command} {}", page.number() + 1);
    }
}
