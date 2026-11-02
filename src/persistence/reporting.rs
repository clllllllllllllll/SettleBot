use crate::domain::{Cents, DisplayName, GameId, GuestId, OccurredOn, PlayerId, SessionId};

use super::{PersistenceError, PersistenceResult};

pub const MAX_REPORT_PAGE_SIZE: usize = 50;
pub const MAX_REPORT_PAGE_NUMBER: u32 = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReportPageRequest {
    number: u32,
    size: usize,
}

impl ReportPageRequest {
    pub fn new(number: u32, size: usize) -> PersistenceResult<Self> {
        if number == 0
            || number > MAX_REPORT_PAGE_NUMBER
            || size == 0
            || size > MAX_REPORT_PAGE_SIZE
        {
            return Err(PersistenceError::InvalidPage);
        }
        let request = Self { number, size };
        request.offset()?;
        Ok(request)
    }

    pub fn number(self) -> u32 {
        self.number
    }

    pub fn size(self) -> usize {
        self.size
    }

    pub fn offset(self) -> PersistenceResult<usize> {
        usize::try_from(self.number - 1)
            .ok()
            .and_then(|page| page.checked_mul(self.size))
            .ok_or(PersistenceError::InvalidPage)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportPage<T> {
    items: Vec<T>,
    number: u32,
    has_more: bool,
}

impl<T> ReportPage<T> {
    pub fn from_extra(mut items: Vec<T>, request: ReportPageRequest) -> Self {
        let has_more = items.len() > request.size();
        items.truncate(request.size());
        Self {
            items,
            number: request.number(),
            has_more,
        }
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }

    pub fn number(&self) -> u32 {
        self.number
    }

    pub fn has_more(&self) -> bool {
        self.has_more
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeaderboardEntry {
    pub player_id: PlayerId,
    pub player_name: DisplayName,
    pub total: Cents,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameLeaderboard {
    pub game_name: DisplayName,
    pub page: ReportPage<LeaderboardEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionSummary {
    pub session_id: SessionId,
    pub game_name: DisplayName,
    pub occurred_on: OccurredOn,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerSessionSummary {
    pub session: SessionSummary,
    pub amount: Cents,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerReport {
    pub player_id: PlayerId,
    pub player_name: DisplayName,
    pub total: Cents,
    pub sessions: ReportPage<PlayerSessionSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReportParticipantIdentity {
    Roster(PlayerId),
    Guest(GuestId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionDetailEntry {
    pub participant: ReportParticipantIdentity,
    pub display_name: DisplayName,
    pub amount: Cents,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionDetail {
    pub session: SessionSummary,
    pub entries: Vec<SessionDetailEntry>,
}

#[allow(async_fn_in_trait)]
pub trait ReportingRepository {
    async fn leaderboard(
        &self,
        request: ReportPageRequest,
    ) -> PersistenceResult<ReportPage<LeaderboardEntry>>;

    async fn game_leaderboard(
        &self,
        game: &GameId,
        request: ReportPageRequest,
    ) -> PersistenceResult<Option<GameLeaderboard>>;

    async fn player_report(
        &self,
        player: &PlayerId,
        request: ReportPageRequest,
    ) -> PersistenceResult<Option<PlayerReport>>;

    async fn session_history(
        &self,
        request: ReportPageRequest,
    ) -> PersistenceResult<ReportPage<SessionSummary>>;

    async fn session_detail(&self, session: &SessionId)
        -> PersistenceResult<Option<SessionDetail>>;
}
