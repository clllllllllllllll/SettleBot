use super::{
    auth::{decode, AuthorizedUpdate},
    commands::{parse, Action, Command},
    flow::{Flow, Stage},
    render::{render, Reply},
    Change, Configuration, Error, Receipt, Repository, Result, Transaction,
};
use crate::domain::{
    CreatedAt, DisplayName, Game, GameId, Player, PlayerId, RosterStatus, SessionId,
};
use crate::persistence::{
    AuditChange, AuditEntry, AuditPageRequest, AuditReason, AuditRevision, AuditTarget, OwnerId,
    Revision,
};

pub trait Clock {
    fn now(&self) -> Result<CreatedAt>;
}

pub(crate) async fn handle<R: Repository, C: Clock>(
    config: &Configuration,
    secret: Option<&str>,
    body: &[u8],
    repository: &R,
    clock: &C,
) -> Result<Reply> {
    config.check_secret(secret)?;
    let update = decode(config, body)?;
    let command = match parse(&update, config.bot_username()) {
        Err(Error::AuditSyntax) => {
            return replay(repository, &update).await?.ok_or(Error::AuditSyntax)
        }
        Ok(Command::Report(report)) => {
            return super::reporting::reply(repository, update.chat, report).await
        }
        command => command?,
    };
    if let Some(reply) = replay(repository, &update).await? {
        return Ok(reply);
    }
    match execute(repository, clock, &update, command).await {
        Ok(reply) => Ok(reply),
        Err(error) => replay(repository, &update).await?.ok_or(error),
    }
}

async fn replay<R: Repository>(repository: &R, update: &AuthorizedUpdate) -> Result<Option<Reply>> {
    let Some(Receipt {
        fingerprint,
        response,
    }) = repository.receipt(&update.key).await?
    else {
        return Ok(None);
    };
    if fingerprint != update.fingerprint() {
        return Err(Error::Rejected);
    }
    Ok(Some(
        serde_json::from_str(&response).map_err(|_| Error::Storage)?,
    ))
}

async fn execute<R: Repository, C: Clock>(
    repository: &R,
    clock: &C,
    update: &AuthorizedUpdate,
    command: Command,
) -> Result<Reply> {
    if let Command::Resume(id) = command {
        let flow = owned_flow(repository, id.as_str(), update.owner, update.chat).await?;
        return render(&flow);
    }
    let change = prepare(repository, clock, update, command).await?;
    let reply = response(&change, update.chat)?;
    let transaction = Transaction {
        key: update.key.clone(),
        fingerprint: update.fingerprint(),
        change,
        reply: reply.clone(),
    };
    repository.apply(&transaction).await?;
    Ok(reply)
}

async fn prepare<R: Repository, C: Clock>(
    repository: &R,
    clock: &C,
    update: &AuthorizedUpdate,
    command: Command,
) -> Result<Change> {
    match command {
        Command::Start => start(repository, update).await,
        Command::Player(name) => add_player(repository, update.number, &name).await,
        Command::Game(name) => add_game(repository, update.number, &name).await,
        Command::Correct {
            session,
            revision,
            amounts,
            reason,
        } => {
            correct(
                repository, clock, update, session, revision, amounts, &reason,
            )
            .await
        }
        Command::Void {
            session,
            revision,
            reason,
        } => void(repository, clock, update, session, revision, &reason).await,
        Command::Step {
            id,
            revision,
            action,
        } => {
            let flow = advance(repository, update, &id, revision, action).await?;
            change(flow, clock)
        }
        Command::Resume(_) => Err(Error::Invalid),
        Command::Report(_) => Err(Error::Invalid),
    }
}

async fn correct(
    repository: &impl Repository,
    clock: &impl Clock,
    update: &AuthorizedUpdate,
    session: SessionId,
    revision: AuditRevision,
    amounts: Vec<crate::domain::Cents>,
    reason: &str,
) -> Result<Change> {
    let detail = repository
        .session_detail(&session, audit_lookup_page()?)
        .await?
        .ok_or(Error::Invalid)?;
    let target = AuditTarget::at_revision(&detail, revision)?;
    let entries = corrected_entries(detail.entries, amounts)?;
    Ok(Change::Audit(AuditChange::correction(
        update.key.clone(),
        target,
        update.owner,
        AuditReason::new(reason)?,
        clock.now()?,
        entries,
    )?))
}

fn corrected_entries(
    entries: Vec<crate::persistence::SessionDetailEntry>,
    amounts: Vec<crate::domain::Cents>,
) -> Result<Vec<AuditEntry>> {
    if entries.len() != amounts.len() {
        return Err(Error::Invalid);
    }
    Ok(entries
        .into_iter()
        .zip(amounts)
        .map(|(entry, amount)| AuditEntry {
            participant: entry.participant,
            display_name: entry.display_name,
            amount,
        })
        .collect())
}

async fn void(
    repository: &impl Repository,
    clock: &impl Clock,
    update: &AuthorizedUpdate,
    session: SessionId,
    revision: AuditRevision,
    reason: &str,
) -> Result<Change> {
    let detail = repository
        .session_detail(&session, audit_lookup_page()?)
        .await?
        .ok_or(Error::Invalid)?;
    Ok(Change::Audit(AuditChange::void(
        update.key.clone(),
        AuditTarget::at_revision(&detail, revision)?,
        update.owner,
        AuditReason::new(reason)?,
        clock.now()?,
    )))
}

fn audit_lookup_page() -> Result<AuditPageRequest> {
    AuditPageRequest::new(0, 1).map_err(Into::into)
}

async fn add_player(repository: &impl Repository, number: u64, name: &str) -> Result<Change> {
    let players = repository.players().await?;
    let active = players
        .iter()
        .filter(|player| player.status() == RosterStatus::Active)
        .count();
    super::policy::catalog_capacity(active)?;
    player(number, name)
}

async fn add_game(repository: &impl Repository, number: u64, name: &str) -> Result<Change> {
    super::policy::catalog_capacity(repository.games().await?.len())?;
    game(number, name)
}

async fn start(repository: &impl Repository, update: &AuthorizedUpdate) -> Result<Change> {
    let games = repository.games().await?;
    let players = repository.players().await?;
    Ok(Change::Flow(Flow::new(
        format!("t{}", update.number),
        update.owner,
        update.chat,
        games,
        players,
    )?))
}

fn player(number: u64, name: &str) -> Result<Change> {
    let id = PlayerId::new(&format!("p{number}"))?;
    Ok(Change::Player(Player::new(
        id,
        DisplayName::new(name)?,
        RosterStatus::Active,
    )))
}

fn game(number: u64, name: &str) -> Result<Change> {
    Ok(Change::Game(Game::new(
        GameId::new(&format!("g{number}"))?,
        DisplayName::new(name)?,
    )))
}

async fn advance(
    repository: &impl Repository,
    update: &AuthorizedUpdate,
    id: &SessionId,
    revision: Revision,
    action: Action,
) -> Result<Flow> {
    let mut flow = owned_flow(repository, id.as_str(), update.owner, update.chat).await?;
    flow.check(update.owner, update.chat, revision)?;
    flow.advance(action)?;
    Ok(flow)
}

async fn owned_flow<R: Repository>(
    repository: &R,
    id: &str,
    owner: OwnerId,
    chat: i64,
) -> Result<Flow> {
    let flow = repository.flow(id).await?.ok_or(Error::Stale)?;
    if flow.owner != owner.value() || flow.chat != chat {
        return Err(Error::Rejected);
    }
    Ok(flow)
}

fn change(flow: Flow, clock: &impl Clock) -> Result<Change> {
    if flow.stage != Stage::Confirmed {
        return Ok(Change::Flow(flow));
    }
    let draft = flow.draft()?;
    draft.check_edit(OwnerId::new(flow.owner)?, Revision::default())?;
    let session = flow.draft()?.confirm(clock.now()?)?;
    Ok(Change::Confirm {
        flow,
        draft: Box::new(draft),
        session: Box::new(session),
    })
}

fn response(change: &Change, chat: i64) -> Result<Reply> {
    match change {
        Change::Flow(flow) | Change::Confirm { flow, .. } => render(flow),
        Change::Player(player) => Ok(added(
            chat,
            "roster player",
            player.name(),
            player.id().as_str(),
        )),
        Change::Game(game) => Ok(added(chat, "game", game.name(), game.id().as_str())),
        Change::Audit(change) => Ok(Reply::text(
            chat,
            format!(
                "Audited {} saved for session {}.",
                change.kind.as_str(),
                change.target.session_id().as_str()
            ),
        )),
    }
}

fn added(chat: i64, kind: &str, name: &DisplayName, id: &str) -> Reply {
    Reply::text(
        chat,
        format!(
            "Added {kind}: {} [{id}]. Start with /session.",
            name.as_str()
        ),
    )
}
