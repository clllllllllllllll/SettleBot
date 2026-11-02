use super::{auth::AuthorizedUpdate, Error, Result};
use crate::domain::{Cents, SessionId};
use crate::persistence::Revision;

pub(super) enum Command {
    Start,
    Player(String),
    Game(String),
    Resume(SessionId),
    Step {
        id: SessionId,
        revision: Revision,
        action: Action,
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
        "/step" | "/guest" => text_step(value, command == "/guest"),
        _ => Err(Error::Invalid),
    }
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
