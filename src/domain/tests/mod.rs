use super::*;

mod dates;
mod sessions;
mod values;

fn session_id() -> SessionId {
    SessionId::new("session-1").expect("valid fixture identifier")
}

fn game_id() -> GameId {
    GameId::new("game-1").expect("valid fixture identifier")
}

fn roster(value: &str) -> Participant {
    Participant::Roster(PlayerId::new(value).expect("valid fixture identifier"))
}

fn guest(session_id: SessionId, local_id: &str, name: &str) -> Guest {
    Guest::new(
        GuestId::new(session_id, local_id).expect("valid fixture identifier"),
        DisplayName::new(name).expect("valid fixture name"),
    )
}

fn draft(participants: Vec<Participant>) -> Result<DraftSession, DomainError> {
    DraftSession::new(
        session_id(),
        game_id(),
        OccurredOn::parse("2026-10-18")?,
        participants,
    )
}

fn entry(participant: &Participant, amount: i64) -> AmountEntry {
    AmountEntry::new(participant.id(), Cents::new(amount))
}

fn created_at() -> CreatedAt {
    CreatedAt::parse("2026-10-20T16:00:00Z").expect("valid fixture instant")
}

fn confirm_roster(amounts: &[i64]) -> Result<ConfirmedSession, DomainError> {
    let participants: Vec<_> = (0..amounts.len())
        .map(|index| roster(&format!("player-{index}")))
        .collect();
    let entries = participants
        .iter()
        .zip(amounts)
        .map(|(participant, amount)| entry(participant, *amount))
        .collect();
    draft(participants)?.confirm(entries, created_at())
}
