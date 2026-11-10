use super::{auth::AuthorizedUpdate, Error, Result};
use crate::domain::{Cents, GameId, PlayerId, SessionId};
use crate::persistence::Revision;

pub(super) enum Command {
    Start,
    Player(String),
    Game(String),
    Resume(SessionId),
    Correct {
        session: SessionId,
        amounts: Vec<Cents>,
        reason: String,
    },
    Void {
        session: SessionId,
        reason: String,
    },
    Report(ReportCommand),
    Step {
        id: SessionId,
        revision: Revision,
        action: Action,
    },
}

pub(super) enum ReportCommand {
    Leaderboard {
        page: u32,
    },
    GameLeaderboard {
        game: GameId,
        page: u32,
    },
    Player {
        player: PlayerId,
        page: u32,
    },
    History {
        page: u32,
    },
    Session {
        session: SessionId,
        after_revision: u64,
    },
}

pub(super) enum Action {
    Game(usize),
    Roster(usize),
    Done,
    Confirm,
    Edit,
    Text(String),
    Guest(String),
}

#[derive(Debug, PartialEq)]
pub(super) enum InputKind {
    Ignore,
    Action,
}

pub(super) fn classify(update: &AuthorizedUpdate, bot_username: &str) -> Result<InputKind> {
    if !update.callback && !update.input.trim_start().starts_with('/') {
        return Ok(InputKind::Ignore);
    }
    validate_address(update, bot_username)?;
    Ok(InputKind::Action)
}

pub(super) fn parse(update: &AuthorizedUpdate, bot_username: &str) -> Result<Command> {
    if update.callback {
        return callback(&update.input);
    }
    let input = update.input.trim();
    let (command, value) = input.split_once(' ').unwrap_or((input, ""));
    let command = addressed_command(command, bot_username)?;
    match command {
        "/session" if value.is_empty() => Ok(Command::Start),
        "/player" => Ok(Command::Player(value.to_owned())),
        "/game" => Ok(Command::Game(value.to_owned())),
        "/resume" => Ok(Command::Resume(SessionId::new(value)?)),
        "/correct" => correction(value),
        "/void" => void(value),
        "/step" | "/guest" => text_step(value, command == "/guest"),
        _ => report(command, value),
    }
}

fn correction(value: &str) -> Result<Command> {
    let (values, reason) = audit_parts(value)?;
    let mut values = values.split_whitespace();
    let session = SessionId::new(values.next().ok_or(Error::Invalid)?)?;
    let amounts = values.map(amount).collect::<Result<Vec<_>>>()?;
    if amounts.is_empty() {
        return Err(Error::Invalid);
    }
    Ok(Command::Correct {
        session,
        amounts,
        reason: reason.to_owned(),
    })
}

fn void(value: &str) -> Result<Command> {
    let (value, reason) = audit_parts(value)?;
    Ok(Command::Void {
        session: SessionId::new(single_value(value)?)?,
        reason: reason.to_owned(),
    })
}

fn audit_parts(value: &str) -> Result<(&str, &str)> {
    let (values, reason) = value.split_once('|').ok_or(Error::Invalid)?;
    let values = values.trim();
    let reason = reason.trim();
    if values.is_empty() || reason.is_empty() {
        return Err(Error::Invalid);
    }
    Ok((values, reason))
}

fn report(command: &str, value: &str) -> Result<Command> {
    let report = match command {
        "/leaderboard" => ReportCommand::Leaderboard {
            page: optional_page(value)?,
        },
        "/gameboard" => return game_leaderboard(value),
        "/stats" => return player_report(value),
        "/history" => ReportCommand::History {
            page: optional_page(value)?,
        },
        "/details" => return session_detail(value),
        _ => return Err(Error::Invalid),
    };
    Ok(Command::Report(report))
}

fn session_detail(value: &str) -> Result<Command> {
    let mut values = value.split_whitespace();
    let session = values.next().ok_or(Error::Invalid)?;
    let after_revision = values
        .next()
        .map(|value| value.parse().map_err(|_| Error::Invalid))
        .transpose()?
        .unwrap_or_default();
    if values.next().is_some() {
        return Err(Error::Invalid);
    }
    Ok(Command::Report(ReportCommand::Session {
        session: SessionId::new(session)?,
        after_revision,
    }))
}

fn game_leaderboard(value: &str) -> Result<Command> {
    let (game, page) = id_and_page(value)?;
    Ok(Command::Report(ReportCommand::GameLeaderboard {
        game: GameId::new(game)?,
        page,
    }))
}

fn player_report(value: &str) -> Result<Command> {
    let (player, page) = id_and_page(value)?;
    Ok(Command::Report(ReportCommand::Player {
        player: PlayerId::new(player)?,
        page,
    }))
}

fn id_and_page(value: &str) -> Result<(&str, u32)> {
    let mut values = value.split_whitespace();
    let id = values.next().ok_or(Error::Invalid)?;
    let page = values.next().map(page).transpose()?.unwrap_or(1);
    if values.next().is_some() {
        return Err(Error::Invalid);
    }
    Ok((id, page))
}

fn optional_page(value: &str) -> Result<u32> {
    if value.is_empty() {
        return Ok(1);
    }
    page(single_value(value)?)
}

fn single_value(value: &str) -> Result<&str> {
    let mut values = value.split_whitespace();
    let value = values.next().ok_or(Error::Invalid)?;
    if values.next().is_some() {
        return Err(Error::Invalid);
    }
    Ok(value)
}

fn page(value: &str) -> Result<u32> {
    let page = value.parse().map_err(|_| Error::Invalid)?;
    if page == 0 {
        return Err(Error::Invalid);
    }
    Ok(page)
}

pub(super) fn validate_address(update: &AuthorizedUpdate, bot_username: &str) -> Result<()> {
    if update.callback {
        return Ok(());
    }
    let input = update.input.trim();
    let command = input.split_once(' ').map_or(input, |(command, _)| command);
    addressed_command(command, bot_username).map(|_| ())
}

fn addressed_command<'a>(command: &'a str, bot_username: &str) -> Result<&'a str> {
    let Some((name, addressed_bot)) = command.split_once('@') else {
        return Ok(command);
    };
    if !addressed_bot.eq_ignore_ascii_case(bot_username) {
        return Err(Error::Rejected);
    }
    Ok(name)
}

fn text_step(value: &str, guest: bool) -> Result<Command> {
    let mut parts = value.splitn(3, ' ');
    let id = SessionId::new(parts.next().ok_or(Error::Invalid)?)?;
    let revision = revision(parts.next().ok_or(Error::Invalid)?)?;
    let value = parts.next().ok_or(Error::Invalid)?.to_owned();
    let action = if guest {
        Action::Guest(value)
    } else {
        Action::Text(value)
    };
    Ok(Command::Step {
        id,
        revision,
        action,
    })
}

fn callback(value: &str) -> Result<Command> {
    let mut parts = value.splitn(3, ':');
    let id = SessionId::new(parts.next().ok_or(Error::Invalid)?)?;
    let revision = revision(parts.next().ok_or(Error::Invalid)?)?;
    let action = callback_action(parts.next().ok_or(Error::Invalid)?)?;
    Ok(Command::Step {
        id,
        revision,
        action,
    })
}

fn callback_action(value: &str) -> Result<Action> {
    match value {
        "done" => Ok(Action::Done),
        "confirm" => Ok(Action::Confirm),
        "edit" => Ok(Action::Edit),
        value if value.starts_with("g:") => Ok(Action::Game(index(&value[2..])?)),
        value if value.starts_with("p:") => Ok(Action::Roster(index(&value[2..])?)),
        _ => Err(Error::Invalid),
    }
}

fn revision(value: &str) -> Result<Revision> {
    value.parse().map(Revision::new).map_err(|_| Error::Invalid)
}

fn index(value: &str) -> Result<usize> {
    value.parse().map_err(|_| Error::Invalid)
}

pub(super) fn amount(value: &str) -> Result<Cents> {
    let sign = match value.as_bytes().first() {
        Some(b'+') => 1_i128,
        Some(b'-') => -1_i128,
        _ => return Err(Error::Invalid),
    };
    let (dollars, cents) = value[1..].split_once('.').ok_or(Error::Invalid)?;
    let magnitude = amount_magnitude(dollars, cents)?;
    let value = magnitude.checked_mul(sign).ok_or(Error::Invalid)?;
    Ok(Cents::new(
        i64::try_from(value).map_err(|_| Error::Invalid)?,
    ))
}

const CENTS_PER_DOLLAR: i128 = 100;
const DECIMAL_PLACES: usize = 2;

fn amount_magnitude(dollars: &str, cents: &str) -> Result<i128> {
    if dollars.is_empty()
        || cents.len() != DECIMAL_PLACES
        || !dollars
            .bytes()
            .chain(cents.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return Err(Error::Invalid);
    }
    let dollars: i128 = dollars.parse().map_err(|_| Error::Invalid)?;
    let cents: i128 = cents.parse().map_err(|_| Error::Invalid)?;
    dollars
        .checked_mul(CENTS_PER_DOLLAR)
        .and_then(|v| v.checked_add(cents))
        .ok_or(Error::Invalid)
}
