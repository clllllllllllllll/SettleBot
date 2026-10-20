import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { after, before, test } from "node:test";
import { createTestHarness } from "wrangler";

// Wrangler owns the real local D1/workerd runtime and migration bookkeeping.
const server = createTestHarness({ workers: [{ configPath: "tests/wrangler.toml" }] });
let worker;
let api;
let database;
const owner = 100;
const created = "2026-10-20T16:00:00Z";

before(async () => {
  await server.listen();
  worker = server.getWorker();
  ({ LEDGER_DB: database } = await worker.getEnv());
  const tables = await database.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'players'").all();
  assert.equal(tables.results.length, 0, "test starts without application tables");
  await worker.applyD1Migrations("LEDGER_DB");
  api = await worker.getExport();
});

after(async () => { await server.close(); });

async function command(input) {
  return JSON.parse(await api.command(JSON.stringify(input)));
}

async function success(input) {
  const result = await command(input);
  assert.ok(!result.error, result.error);
  return result.ok;
}

function selection(id, participants = balanced()) {
  return { id, owner, game: "poker", occurred_on: "2026-10-19", revision: 0, participants };
}

function balanced() {
  return [
    { kind: "roster", id: "player_one", amount: "250" },
    { kind: "guest", id: "player_one", name: "Sample Guest", amount: "-250" },
  ];
}

function confirm(id, update_key, overrides = {}) {
  return { operation: "confirm", id, update_key, owner, revision: 0, created_at: created, ...overrides };
}

async function save(draft, expected_revision) {
  return success({ operation: "save_draft", draft, expected_revision });
}

async function resume(id, requestedOwner = owner) {
  return success({ operation: "resume", id, owner: requestedOwner });
}

async function counts(id) {
  const row = await database.prepare(`SELECT
    (SELECT COUNT(*) FROM sessions WHERE id = ?1) AS sessions,
    (SELECT COUNT(*) FROM session_entries WHERE session_id = ?1) AS entries,
    (SELECT COUNT(*) FROM processed_updates WHERE session_id = ?1) AS updates`).bind(id).first();
  return { ...row };
}

test("migrations create an empty ledger and seed only Mahjong/Poker idempotently", async () => {
  assert.deepEqual(await success({ operation: "catalog" }), {
    players: [], games: [{ id: "mahjong", name: "Mahjong" }, { id: "poker", name: "Poker" }],
  });
  await worker.applyD1Migrations("LEDGER_DB");
  const seed = await readFile(new URL("../migrations/0002_initial_games.sql", import.meta.url), "utf8");
  await database.prepare(seed).run();
  await database.prepare(seed).run();
  assert.equal((await success({ operation: "catalog" })).games.length, 2);
  assert.equal((await database.prepare("SELECT COUNT(*) AS count FROM d1_migrations").first()).count, 2);
});

test("stable IDs are unique while repeated display names and case-distinct IDs are allowed", async () => {
  for (const id of ["player_one", "player_two", "unrelated", "PLAYER_ONE"]) {
    await success({ operation: "add_player", id, name: "Sample Player" });
  }
  assert.equal((await command({ operation: "add_player", id: "player_one", name: "Other Name" })).error, "IdentityConflict");
  await success({ operation: "add_game", id: "sample_game", name: "Poker" });
  assert.equal((await command({ operation: "add_game", id: "sample_game", name: "Other" })).error, "IdentityConflict");
  assert.equal((await success({ operation: "catalog" })).players.length, 4);
});

test("partial draft survives fresh repository calls and resumes with owner, date, guests and amounts", async () => {
  const draft = selection("resume", [{ kind: "roster", id: "player_one" }, { kind: "guest", id: "visitor", name: "Sample Guest", amount: "-250" }]);
  const original = await save(draft);
  api = await worker.getExport();
  assert.deepEqual(await resume("resume"), original);
  assert.equal(original.participants[0].amount, null);
  assert.equal(original.participants[1].session, "resume");
  assert.equal((await command({ operation: "resume", id: "resume", owner: owner + 1 })).error, "OwnerMismatch");
  const edited = { ...draft, revision: 1, participants: balanced() };
  await save(edited, 0);
  assert.equal((await resume("resume")).revision, 1);
  assert.equal((await command({ operation: "save_draft", draft: edited, expected_revision: 0 })).error, "RevisionConflict");
  assert.equal((await command({ operation: "save_draft", draft: { ...edited, owner: owner + 1, revision: 2 }, expected_revision: 1 })).error, "OwnerMismatch");
});

test("duplicate selections and foreign guests are rejected without a persisted draft", async () => {
  const participant = balanced()[0];
  const duplicate = await command({ operation: "save_draft", draft: selection("duplicate", [participant, participant]) });
  assert.match(duplicate.error, /DuplicateParticipant/);
  const guest = { ...balanced()[1], guest_session: "another_session" };
  assert.match((await command({ operation: "save_draft", draft: selection("foreign_guest", [participant, guest]) })).error, /GuestOutsideSession/);
  await assert.rejects(database.prepare("INSERT INTO draft_participants (draft_id, kind, identity_id, roster_player_id, position) VALUES ('resume', 'roster', 'player_one', 'player_one', 4)").run());
});

test("confirmation persists guests, selected players, date, consumption and stable replay result", async () => {
  await save(selection("confirmed"));
  assert.equal((await command(confirm("confirmed", "unauthorized", { owner: owner + 1 }))).error, "OwnerMismatch");
  assert.deepEqual(await counts("confirmed"), { sessions: 0, entries: 0, updates: 0 });
  const result = await success(confirm("confirmed", "update_confirmed"));
  const replay = await success(confirm("confirmed", "update_confirmed", { created_at: "2027-01-01T00:00:00Z", revision: 99 }));
  assert.deepEqual(replay, result);
  const persisted = await success({ operation: "session", id: "confirmed" });
  assert.equal(persisted.occurred_on, "2026-10-19");
  assert.equal(persisted.created_at, created);
  assert.equal(persisted.participants.length, 2);
  assert.deepEqual(await counts("confirmed"), { sessions: 1, entries: 2, updates: 1 });
  assert.equal((await resume("confirmed")).state, "Consumed");
  assert.equal((await resume("confirmed")).revision, 1);
  assert.equal((await command(confirm("confirmed", "different_update"))).error, "DraftConsumed");
  assert.equal((await command(confirm("resume", "update_confirmed"))).error, "UpdateKeyConflict");
  assert.equal((await command(confirm("confirmed", "update_confirmed", { owner: owner + 1 }))).error, "UpdateKeyConflict");
  assert.equal((await database.prepare("SELECT COUNT(*) AS count FROM session_entries WHERE roster_player_id IN ('unrelated', 'player_two', 'PLAYER_ONE')").first()).count, 0);
  assert.equal((await success({ operation: "catalog" })).players.length, 4, "guest was never promoted into roster");
});

test("two different concurrent updates create exactly one session", async () => {
  await save(selection("race"));
  const results = await Promise.all([command(confirm("race", "race_one")), command(confirm("race", "race_two"))]);
  assert.equal(results.filter(result => result.ok).length, 1);
  assert.deepEqual(results.filter(result => result.error).map(result => result.error), ["DraftConsumed"]);
  assert.deepEqual(await counts("race"), { sessions: 1, entries: 2, updates: 1 });
});

test("concurrent delivery of the same key replays the single original success", async () => {
  await save(selection("same_race"));
  const results = await Promise.all([
    success(confirm("same_race", "same_key")),
    success(confirm("same_race", "same_key", { created_at: "2026-10-21T00:00:00Z" })),
  ]);
  assert.deepEqual(results[0], results[1]);
  assert.deepEqual(await counts("same_race"), { sessions: 1, entries: 2, updates: 1 });
});

test("concurrent draft revisions cannot overwrite a winning edit", async () => {
  await save(selection("edit_race"));
  const left = { ...selection("edit_race"), revision: 1, occurred_on: "2026-10-01" };
  const right = { ...selection("edit_race", []), revision: 1, occurred_on: "2026-10-02" };
  const results = await Promise.all([
    command({ operation: "save_draft", draft: left, expected_revision: 0 }),
    command({ operation: "save_draft", draft: right, expected_revision: 0 }),
  ]);
  assert.equal(results.filter(result => result.ok).length, 1);
  assert.deepEqual(results.filter(result => result.error).map(result => result.error), ["RevisionConflict"]);
  assert.deepEqual(await resume("edit_race"), results.find(result => result.ok).ok);
});

test("incomplete, nonzero and stale confirmations leave the draft and ledger untouched", async () => {
  await save(selection("incomplete", [{ kind: "roster", id: "player_one", amount: "1" }, { kind: "roster", id: "player_two" }]));
  assert.match((await command(confirm("incomplete", "incomplete_key"))).error, /MissingAmount/);
  await save(selection("nonzero", [{ kind: "roster", id: "player_one", amount: "1" }, { kind: "roster", id: "player_two", amount: "1" }]));
  assert.match((await command(confirm("nonzero", "nonzero_key"))).error, /NonZeroTotal/);
  assert.equal((await command(confirm("nonzero", "stale_key", { revision: 1 }))).error, "RevisionConflict");
  assert.deepEqual(await counts("incomplete"), { sessions: 0, entries: 0, updates: 0 });
  assert.deepEqual(await counts("nonzero"), { sessions: 0, entries: 0, updates: 0 });
  assert.equal((await resume("nonzero")).state, "Open");
});

test("failure at the last write rolls back ledger, consumed draft and success marker; retry succeeds", async () => {
  await save(selection("rollback"));
  const original = await resume("rollback");
  await database.prepare("CREATE TRIGGER fail_success BEFORE INSERT ON processed_updates WHEN NEW.update_key = 'rollback_key' BEGIN SELECT RAISE(ABORT, 'injected final write failure'); END").run();
  assert.equal((await command(confirm("rollback", "rollback_key"))).error, "Storage");
  assert.deepEqual(await counts("rollback"), { sessions: 0, entries: 0, updates: 0 });
  assert.deepEqual(await resume("rollback"), original);
  await database.prepare("DROP TRIGGER fail_success").run();
  const result = await success(confirm("rollback", "rollback_key"));
  assert.deepEqual(await success(confirm("rollback", "rollback_key", { created_at: "2026-10-22T00:00:00Z" })), result, "safe after a lost response");
  assert.deepEqual(await counts("rollback"), { sessions: 1, entries: 2, updates: 1 });
});

test("failure during participant insertion rolls back preceding session and entries", async () => {
  await save(selection("entry_failure"));
  await database.prepare("CREATE TRIGGER fail_entry BEFORE INSERT ON session_entries WHEN NEW.session_id = 'entry_failure' AND NEW.position = 1 BEGIN SELECT RAISE(ABORT, 'injected entry failure'); END").run();
  assert.equal((await command(confirm("entry_failure", "entry_failure_key"))).error, "Storage");
  assert.deepEqual(await counts("entry_failure"), { sessions: 0, entries: 0, updates: 0 });
  assert.equal((await resume("entry_failure")).state, "Open");
  await database.prepare("DROP TRIGGER fail_entry").run();
  await success(confirm("entry_failure", "entry_failure_key"));
});

test("full i64 cents round trip through D1 without JS numeric rounding", async () => {
  const participants = [
    { kind: "roster", id: "player_one", amount: "9223372036854775807" },
    { kind: "guest", id: "low", name: "Sample Guest", amount: "-9223372036854775808" },
    { kind: "roster", id: "player_two", amount: "1" },
  ];
  await save(selection("limits", participants));
  assert.deepEqual((await resume("limits")).participants.map(value => value.amount), participants.map(value => value.amount));
  await success(confirm("limits", "limits_key"));
  assert.deepEqual((await success({ operation: "session", id: "limits" })).participants.map(value => value.amount), participants.map(value => value.amount));
});

test("malformed persisted values are rejected and storage failures are typed", async () => {
  await save(selection("malformed"));
  await database.prepare("UPDATE drafts SET occurred_on = '2026-02-30' WHERE id = 'malformed'").run();
  assert.match((await command({ operation: "resume", id: "malformed", owner })).error, /^Corrupt/);
  assert.equal((await command(confirm("missing", "missing_key"))).error, "NotFound");
  const draft = selection("missing_player", [{ kind: "roster", id: "not_registered", amount: "0" }]);
  assert.equal((await command({ operation: "save_draft", draft })).error, "Storage");
  assert.equal((await database.prepare("SELECT COUNT(*) AS count FROM drafts WHERE id = 'missing_player'").first()).count, 0);
  await database.prepare("UPDATE session_entries SET amount_cents = 99 WHERE session_id = 'confirmed' AND kind = 'guest'").run();
  assert.match((await command({ operation: "session", id: "confirmed" })).error, /^Corrupt/);
  await assert.rejects(database.prepare("UPDATE session_entries SET amount_cents = 'invalid' WHERE session_id = 'confirmed'").run());
});
