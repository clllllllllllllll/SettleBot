-- Stable identifiers, not display names, define identity. Names may repeat.
CREATE TABLE players (
    id TEXT PRIMARY KEY NOT NULL,
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) > 0),
    status TEXT NOT NULL CHECK (status IN ('active', 'inactive'))
) STRICT;

CREATE TABLE games (
    id TEXT PRIMARY KEY NOT NULL,
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) > 0)
) STRICT;

CREATE TABLE drafts (
    id TEXT PRIMARY KEY NOT NULL,
    owner_id TEXT NOT NULL,
    game_id TEXT NOT NULL REFERENCES games(id),
    occurred_on TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision BETWEEN 0 AND 4294967295),
    state TEXT NOT NULL CHECK (state IN ('open', 'consumed'))
) STRICT;

CREATE TABLE draft_participants (
    draft_id TEXT NOT NULL REFERENCES drafts(id),
    kind TEXT NOT NULL CHECK (kind IN ('roster', 'guest')),
    identity_id TEXT NOT NULL,
    roster_player_id TEXT REFERENCES players(id),
    guest_name TEXT,
    position INTEGER NOT NULL CHECK (position >= 0),
    amount_cents INTEGER,
    PRIMARY KEY (draft_id, kind, identity_id),
    UNIQUE (draft_id, position),
    CHECK ((kind = 'roster' AND roster_player_id IS NOT NULL
            AND roster_player_id = identity_id AND guest_name IS NULL)
        OR (kind = 'guest' AND roster_player_id IS NULL
            AND guest_name IS NOT NULL AND length(trim(guest_name)) > 0))
) STRICT;

CREATE TABLE sessions (
    id TEXT PRIMARY KEY NOT NULL REFERENCES drafts(id),
    owner_id TEXT NOT NULL,
    game_id TEXT NOT NULL REFERENCES games(id),
    occurred_on TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE session_entries (
    session_id TEXT NOT NULL REFERENCES sessions(id),
    kind TEXT NOT NULL CHECK (kind IN ('roster', 'guest')),
    identity_id TEXT NOT NULL,
    roster_player_id TEXT REFERENCES players(id),
    guest_name TEXT,
    position INTEGER NOT NULL CHECK (position >= 0),
    amount_cents INTEGER NOT NULL,
    PRIMARY KEY (session_id, kind, identity_id),
    UNIQUE (session_id, position),
    CHECK ((kind = 'roster' AND roster_player_id IS NOT NULL
            AND roster_player_id = identity_id AND guest_name IS NULL)
        OR (kind = 'guest' AND roster_player_id IS NULL
            AND guest_name IS NOT NULL AND length(trim(guest_name)) > 0))
) STRICT;

CREATE TABLE processed_updates (
    update_key TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL UNIQUE REFERENCES sessions(id)
) STRICT;

CREATE INDEX drafts_by_owner ON drafts(owner_id, state);
CREATE INDEX entries_by_player ON session_entries(roster_player_id, session_id)
    WHERE roster_player_id IS NOT NULL;
CREATE INDEX sessions_by_date ON sessions(occurred_on, id);
