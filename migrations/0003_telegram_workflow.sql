-- Partial guided selections are durable before a complete ledger draft exists.
CREATE TABLE telegram_flows (
    id TEXT PRIMARY KEY NOT NULL,
    owner_id TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision BETWEEN 0 AND 4294967295),
    state TEXT NOT NULL CHECK (state IN ('open', 'consumed')),
    document TEXT NOT NULL CHECK (json_valid(document))
) STRICT;

-- Every successful mutation commits its replay response in the same D1 batch.
-- Confirmation also retains the Phase 2 processed_updates record.
CREATE TABLE telegram_updates (
    update_key TEXT PRIMARY KEY NOT NULL,
    fingerprint TEXT NOT NULL,
    response TEXT NOT NULL CHECK (json_valid(response))
) STRICT;
