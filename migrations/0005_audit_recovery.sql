-- Corrections and voids are append-only. The original sessions and entries are
-- never updated or deleted; reports resolve the latest audit event in scope.
CREATE TABLE session_audits (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    revision INTEGER NOT NULL CHECK (revision > 0),
    kind TEXT NOT NULL CHECK (kind IN ('correction', 'void')),
    actor_id TEXT NOT NULL,
    reason TEXT NOT NULL CHECK (
        length(trim(reason)) BETWEEN 1 AND 256
        AND instr(reason, char(10)) = 0
        AND instr(reason, char(13)) = 0
    ),
    created_at TEXT NOT NULL,
    UNIQUE (session_id, revision)
) STRICT;

CREATE TABLE session_audit_entries (
    audit_id TEXT NOT NULL REFERENCES session_audits(id),
    kind TEXT NOT NULL CHECK (kind IN ('roster', 'guest')),
    identity_id TEXT NOT NULL,
    roster_player_id TEXT REFERENCES players(id),
    guest_name TEXT,
    position INTEGER NOT NULL CHECK (position >= 0),
    amount_cents INTEGER NOT NULL,
    PRIMARY KEY (audit_id, kind, identity_id),
    UNIQUE (audit_id, position),
    CHECK ((kind = 'roster' AND roster_player_id IS NOT NULL
            AND roster_player_id = identity_id AND guest_name IS NULL)
        OR (kind = 'guest' AND roster_player_id IS NULL
            AND guest_name IS NOT NULL AND length(trim(guest_name)) > 0))
) STRICT;

CREATE INDEX session_audits_by_session
    ON session_audits(session_id, revision DESC);
CREATE INDEX audit_entries_by_player
    ON session_audit_entries(roster_player_id, audit_id)
    WHERE roster_player_id IS NOT NULL;
