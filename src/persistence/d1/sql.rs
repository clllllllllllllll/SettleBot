pub const PLAYERS: &str = "SELECT id, display_name, status FROM players ORDER BY id";
pub const GAMES: &str = "SELECT id, display_name FROM games ORDER BY id";
pub const ADD_PLAYER: &str = "
    INSERT INTO players (id, display_name, status) VALUES (?, ?, ?)
    ON CONFLICT(id) DO NOTHING";
pub const ADD_GAME: &str = "
    INSERT INTO games (id, display_name) VALUES (?, ?)
    ON CONFLICT(id) DO NOTHING";
pub const DRAFT: &str = "
    SELECT id, owner_id, game_id, occurred_on, revision, state FROM drafts WHERE id = ?";
pub const DRAFT_PARTICIPANTS: &str = "
    SELECT kind, identity_id, roster_player_id, guest_name, position,
           CAST(amount_cents AS TEXT) AS amount_cents
    FROM draft_participants WHERE draft_id = ? ORDER BY position";
pub const CREATE_DRAFT: &str = "
    INSERT INTO drafts (id, owner_id, game_id, occurred_on, revision, state)
    VALUES (?, ?, ?, ?, 0, 'open')";

// A stale revision/owner/state forces a CHECK failure and rolls back every statement.
pub const REPLACE_DRAFT: &str = "
    UPDATE drafts SET game_id = ?3, occurred_on = ?4,
        revision = CASE
            WHEN owner_id = ?2 AND revision = ?5 AND state = 'open' THEN ?6
            ELSE -1 END
    WHERE id = ?1";
pub const CLEAR_PARTICIPANTS: &str = "DELETE FROM draft_participants WHERE draft_id = ?";
pub const ADD_DRAFT_PARTICIPANT: &str = "
    INSERT INTO draft_participants
        (draft_id, kind, identity_id, roster_player_id, guest_name, position, amount_cents)
    VALUES (?, ?, ?, ?, ?, CAST(? AS INTEGER), CAST(? AS INTEGER))";
pub const SESSION: &str = "
    SELECT id, owner_id, game_id, occurred_on, created_at FROM sessions WHERE id = ?";
pub const SESSION_ENTRIES: &str = "
    SELECT kind, identity_id, roster_player_id, guest_name, position,
           CAST(amount_cents AS TEXT) AS amount_cents
    FROM session_entries WHERE session_id = ? ORDER BY position";

// This conditional insert is the transaction's optimistic lock. When it does not
// insert, the following mandatory entries fail their FK or uniqueness checks.
pub const CONFIRM_SESSION: &str = "
    INSERT INTO sessions (id, owner_id, game_id, occurred_on, created_at)
    SELECT id, owner_id, game_id, occurred_on, ?4 FROM drafts
    WHERE id = ?1 AND owner_id = ?2 AND revision = ?3 AND state = 'open'";
pub const ADD_SESSION_ENTRY: &str = "
    INSERT INTO session_entries
        (session_id, kind, identity_id, roster_player_id, guest_name, position, amount_cents)
    VALUES (?, ?, ?, ?, ?, CAST(? AS INTEGER), CAST(? AS INTEGER))";
pub const CONSUME_DRAFT: &str = "
    UPDATE drafts SET state = 'consumed', revision = revision + 1 WHERE id = ?";
pub const PROCESS_UPDATE: &str = "
    INSERT INTO processed_updates (update_key, session_id) VALUES (?, ?)";
pub const PROCESSED_UPDATE: &str = "
    SELECT sessions.id, sessions.owner_id, sessions.game_id,
           sessions.occurred_on, sessions.created_at
    FROM processed_updates JOIN sessions ON sessions.id = processed_updates.session_id
    WHERE update_key = ?";
