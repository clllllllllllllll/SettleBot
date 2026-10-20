use super::*;

#[test]
fn identifiers_require_nonempty_values() {
    for value in ["", " ", "\n\t"] {
        assert_identifier_error(value, DomainError::RequiredValue);
    }
}

#[test]
fn identifiers_reject_non_token_characters() {
    for value in [" leading", "trailing ", "two words", "a/b", "名字", "a\0b"] {
        assert_identifier_error(value, DomainError::InvalidIdentifier);
    }
}

fn assert_identifier_error(value: &str, error: fn(&'static str) -> DomainError) {
    let results = [
        (PlayerId::new(value).map(|_| ()), "player_id"),
        (GameId::new(value).map(|_| ()), "game_id"),
        (SessionId::new(value).map(|_| ()), "session_id"),
        (GuestId::new(session_id(), value).map(|_| ()), "guest_id"),
    ];
    for (result, field) in results {
        assert_eq!(result, Err(error(field)));
    }
}

#[test]
fn identifiers_preserve_case_and_stable_values() -> Result<(), DomainError> {
    let player = PlayerId::new("Player_A-01")?;
    assert_eq!(player.as_str(), "Player_A-01");
    assert_ne!(player, PlayerId::new("player_A-01")?);
    assert_eq!(GameId::new("game-1")?.as_str(), "game-1");
    assert_eq!(session_id().as_str(), "session-1");
    assert_eq!(GuestId::new(session_id(), "guest_1")?.as_str(), "guest_1");
    Ok(())
}

#[test]
fn names_require_visible_values_without_control_characters() -> Result<(), DomainError> {
    for value in ["", "  ", "\t\n"] {
        assert_eq!(
            DisplayName::new(value),
            Err(DomainError::RequiredValue("display_name"))
        );
    }
    for value in ["Player\nA", "Player\0A", "Game\tA"] {
        assert_eq!(
            DisplayName::new(value),
            Err(DomainError::InvalidDisplayName)
        );
    }
    assert_eq!(DisplayName::new("  Player A  ")?.as_str(), "Player A");
    assert_eq!(DisplayName::new("访客")?.as_str(), "访客");
    Ok(())
}

#[test]
fn players_and_games_compose_validated_values() -> Result<(), DomainError> {
    let player_id = PlayerId::new("player-a")?;
    let name = DisplayName::new("Player A")?;
    let player = Player::new(player_id.clone(), name.clone(), RosterStatus::Active);
    let inactive = Player::new(player_id.clone(), name.clone(), RosterStatus::Inactive);
    let game = Game::new(game_id(), DisplayName::new("Game A")?);
    assert_eq!(player.id(), &player_id);
    assert_eq!(player.name(), &name);
    assert_eq!(player.status(), RosterStatus::Active);
    assert_eq!(inactive.status(), RosterStatus::Inactive);
    assert_eq!(game.id(), &game_id());
    assert_eq!(game.name().as_str(), "Game A");
    Ok(())
}

#[test]
fn money_preserves_every_signed_cent_boundary() {
    for value in [i64::MIN, -1, 0, 1, i64::MAX] {
        assert_eq!(Cents::new(value).value(), value);
    }
    assert_eq!(Cents::default(), Cents::ZERO);
}

#[test]
fn sgd_formatting_handles_signs_cents_and_numeric_boundaries() {
    let cases = [
        (0, "S$0.00"),
        (1, "S$0.01"),
        (-1, "-S$0.01"),
        (100, "S$1.00"),
        (-12345, "-S$123.45"),
        (i64::MAX, "S$92233720368547758.07"),
        (i64::MIN, "-S$92233720368547758.08"),
    ];
    for (value, expected) in cases {
        assert_eq!(Cents::new(value).to_string(), expected);
    }
}

#[test]
fn sums_reject_positive_and_negative_overflow() {
    for amounts in [[i64::MAX, 1], [i64::MIN, -1]] {
        assert_eq!(
            Cents::checked_sum(amounts.map(Cents::new)),
            Err(DomainError::AmountOverflow)
        );
    }
}

#[test]
fn sums_do_not_depend_on_intermediate_ordering() -> Result<(), DomainError> {
    for amounts in [[i64::MAX, 1, i64::MIN], [i64::MIN, 1, i64::MAX]] {
        assert_eq!(Cents::checked_sum(amounts.map(Cents::new))?, Cents::ZERO);
    }
    assert_eq!(Cents::checked_sum([])?, Cents::ZERO);
    Ok(())
}
