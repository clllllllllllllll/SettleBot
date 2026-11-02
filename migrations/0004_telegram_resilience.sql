-- One row per verified Telegram update keeps duplicate delivery idempotent while
-- bounding new administrator requests in a rolling low-traffic window.
CREATE TABLE telegram_rate_events (
    update_key TEXT PRIMARY KEY NOT NULL,
    owner_id TEXT NOT NULL,
    occurred_at INTEGER NOT NULL
) STRICT;

CREATE INDEX telegram_rate_events_owner_time
    ON telegram_rate_events (owner_id, occurred_at);

-- Application checks return a friendly limit before creation. These triggers
-- preserve the same 24-choice invariant if concurrent requests race that check.
CREATE TRIGGER active_player_catalog_capacity
BEFORE INSERT ON players
WHEN NEW.status = 'active' AND (
    SELECT COUNT(*) FROM players WHERE status = 'active'
) >= 24
BEGIN
    SELECT RAISE(ABORT, 'active player catalog capacity reached');
END;

CREATE TRIGGER active_player_catalog_capacity_on_update
BEFORE UPDATE OF status ON players
WHEN OLD.status <> 'active' AND NEW.status = 'active' AND (
    SELECT COUNT(*) FROM players WHERE status = 'active'
) >= 24
BEGIN
    SELECT RAISE(ABORT, 'active player catalog capacity reached');
END;

CREATE TRIGGER game_catalog_capacity
BEFORE INSERT ON games
WHEN (SELECT COUNT(*) FROM games) >= 24
BEGIN
    SELECT RAISE(ABORT, 'game catalog capacity reached');
END;
