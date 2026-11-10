import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createTestHarness } from "wrangler";

const server = createTestHarness({ workers: [{ configPath: "tests/wrangler.toml" }] });
let api;
let database;
let sequence = 30000;
const created = "2026-11-01T16:00:00Z";

before(async () => {
  await server.listen();
  const worker = server.getWorker();
  ({ LEDGER_DB: database } = await worker.getEnv());
  await worker.applyD1Migrations("LEDGER_DB");
  api = await worker.getExport();
  await seedLedger();
});

after(async () => { await server.close(); });

async function command(input) {
  return JSON.parse(await api.command(JSON.stringify(input)));
}

async function commandSuccess(input) {
  const result = await command(input);
  assert.ok(!result.error, result.error);
  return result.ok;
}

async function seedLedger() {
  for (const [id, name] of [["alpha", "Alpha Player"], ["beta", "Beta Player"], ["idle", "Idle Player"]]) {
    await commandSuccess({ operation: "add_player", id, name });
  }
  await saveAndConfirm("session_one", "poker", "2026-10-03", [
    { kind: "roster", id: "alpha", amount: "1000" },
    { kind: "roster", id: "beta", amount: "-600" },
    { kind: "guest", id: "visitor", name: "Sample Visitor", amount: "-400" },
  ]);
  await saveAndConfirm("session_two", "mahjong", "2026-10-02", [
    { kind: "roster", id: "alpha", amount: "-250" },
    { kind: "roster", id: "beta", amount: "250" },
  ]);
}

async function saveAndConfirm(id, game, occurred_on, participants) {
  await commandSuccess({
    operation: "save_draft",
    draft: { id, owner: 100, game, occurred_on, revision: 0, participants },
  });
  await commandSuccess({
    operation: "confirm",
    id,
    owner: 100,
    revision: 0,
    update_key: `confirm_${id}`,
    created_at: created,
  });
}

async function seedBatchedBoundaryHistory() {
  await commandSuccess({ operation: "add_player", id: "batched", name: "Batched Player" });
  const amount = "CASE n WHEN 0 THEN 9223372036854775807 WHEN 100 THEN 1 WHEN 101 THEN -9223372036854775807 ELSE 0 END";
  const counterAmount = "CASE n WHEN 0 THEN -9223372036854775807 WHEN 100 THEN -1 WHEN 101 THEN 9223372036854775807 ELSE 0 END";
  const sequence = "WITH RECURSIVE numbers(n) AS (SELECT 0 UNION ALL SELECT n + 1 FROM numbers WHERE n < 101)";
  await database.batch([
    database.prepare(`${sequence}
      INSERT INTO drafts (id, owner_id, game_id, occurred_on, revision, state)
      SELECT printf('batch_%03d', n), '100', 'poker', '2026-08-01', 1, 'consumed' FROM numbers`),
    database.prepare(`${sequence}
      INSERT INTO sessions (id, owner_id, game_id, occurred_on, created_at)
      SELECT printf('batch_%03d', n), '100', 'poker', '2026-08-01', '${created}' FROM numbers`),
    database.prepare(`${sequence}
      INSERT INTO session_entries
        (session_id, kind, identity_id, roster_player_id, guest_name, position, amount_cents)
      SELECT printf('batch_%03d', n), 'roster', 'batched', 'batched', NULL, 0, ${amount}
      FROM numbers`),
    database.prepare(`${sequence}
      INSERT INTO session_entries
        (session_id, kind, identity_id, roster_player_id, guest_name, position, amount_cents)
      SELECT printf('batch_%03d', n), 'guest', 'counterparty', NULL, 'Sample Guest', 1,
             ${counterAmount}
      FROM numbers`),
  ]);
}

function message(text, owner = 100) {
  return {
    update_id: sequence++,
    message: {
      date: 1,
      message_id: sequence,
      chat: { id: -100, type: "supergroup" },
      from: { id: owner, is_bot: false },
      text,
    },
  };
}

async function telegram(text, owner = 100) {
  const input = { secret: "fake_secret", body: message(text, owner), created_at: created };
  return JSON.parse(await api.telegram(JSON.stringify(input)));
}

async function telegramSuccess(text) {
  const result = await telegram(text);
  assert.ok(!result.error, result.error);
  return result.ok;
}

test("report queries exclude guests, preserve zero totals, and paginate deterministically", async () => {
  const first = await commandSuccess({ operation: "leaderboard", game: null, page: 1, size: 2 });
  const second = await commandSuccess({ operation: "leaderboard", game: null, page: 2, size: 2 });
  assert.deepEqual(first, {
    number: 1,
    has_more: true,
    items: [
      { id: "alpha", name: "Alpha Player", total: "750" },
      { id: "idle", name: "Idle Player", total: "0" },
    ],
  });
  assert.deepEqual(second, {
    number: 2,
    has_more: false,
    items: [{ id: "beta", name: "Beta Player", total: "-350" }],
  });
  assert.deepEqual([...first.items, ...second.items].map(row => row.id), ["alpha", "idle", "beta"]);
  assert.ok(!JSON.stringify([first, second]).includes("visitor"));

  const poker = await commandSuccess({ operation: "leaderboard", game: "poker", page: 1, size: 3 });
  assert.deepEqual(poker.page.items.map(row => [row.id, row.total]), [
    ["alpha", "1000"], ["idle", "0"], ["beta", "-600"],
  ]);

  const player = await commandSuccess({ operation: "player_report", player: "alpha", page: 1, size: 2 });
  assert.equal(player.total, "750");
  assert.deepEqual(player.sessions.items.map(row => [row.id, row.occurred_on, row.amount]), [
    ["session_one", "2026-10-03", "1000"],
    ["session_two", "2026-10-02", "-250"],
  ]);
  assert.ok(!JSON.stringify(player).includes("Sample Visitor"));

  const historyOne = await commandSuccess({ operation: "history", page: 1, size: 1 });
  const historyTwo = await commandSuccess({ operation: "history", page: 2, size: 1 });
  assert.deepEqual([...historyOne.items, ...historyTwo.items].map(row => row.id), ["session_one", "session_two"]);
  assert.equal(historyOne.has_more, true);
  assert.equal(historyTwo.has_more, false);
  assert.ok(!JSON.stringify([historyOne, historyTwo]).includes("Sample Visitor"));

  const detail = await commandSuccess({ operation: "details", id: "session_one" });
  assert.deepEqual(detail.entries.at(-1), {
    kind: "guest", id: "visitor", name: "Sample Visitor", amount: "-400",
  });
  assert.equal((await command({ operation: "leaderboard", game: null, page: 0, size: 2 })).error, "InvalidPage");
  assert.equal((await command({ operation: "leaderboard", game: null, page: 10001, size: 2 })).error, "InvalidPage");
  assert.equal((await command({ operation: "leaderboard", game: null, page: 1, size: 51 })).error, "InvalidPage");
});

test("report totals avoid SQLite intermediate integer overflow", async () => {
  await commandSuccess({ operation: "add_player", id: "boundary", name: "Boundary Player" });
  await saveAndConfirm("overflow_a", "poker", "2026-09-03", [
    { kind: "roster", id: "boundary", amount: "9223372036854775807" },
    { kind: "guest", id: "counterparty", name: "Sample Guest", amount: "-9223372036854775807" },
  ]);
  await saveAndConfirm("overflow_b", "poker", "2026-09-02", [
    { kind: "roster", id: "boundary", amount: "1" },
    { kind: "guest", id: "counterparty", name: "Sample Guest", amount: "-1" },
  ]);
  await saveAndConfirm("overflow_c", "poker", "2026-09-01", [
    { kind: "roster", id: "boundary", amount: "-9223372036854775807" },
    { kind: "guest", id: "counterparty", name: "Sample Guest", amount: "9223372036854775807" },
  ]);

  const overall = await commandSuccess({ operation: "leaderboard", game: null, page: 1, size: 50 });
  const poker = await commandSuccess({ operation: "leaderboard", game: "poker", page: 1, size: 50 });
  const player = await commandSuccess({ operation: "player_report", player: "boundary", page: 1, size: 10 });
  assert.equal(overall.items.find(row => row.id === "boundary").total, "1");
  assert.equal(poker.page.items.find(row => row.id === "boundary").total, "1");
  assert.equal(player.total, "1");
  assert.deepEqual(player.sessions.items.map(row => row.amount), [
    "9223372036854775807", "1", "-9223372036854775807",
  ]);
});

test("report aggregation spans bounded batches and keeps one snapshot", async () => {
  await seedBatchedBoundaryHistory();

  const overall = await commandSuccess({ operation: "leaderboard", game: null, page: 1, size: 50 });
  const poker = await commandSuccess({ operation: "leaderboard", game: "poker", page: 1, size: 50 });
  const player = await commandSuccess({ operation: "player_report", player: "batched", page: 1, size: 50 });
  assert.equal(overall.items.find(row => row.id === "batched").total, "1");
  assert.equal(poker.page.items.find(row => row.id === "batched").total, "1");
  assert.equal(player.total, "1");
  assert.equal(player.sessions.items.length, 50);
  assert.equal(player.sessions.has_more, true);

  const snapshot = await commandSuccess({ operation: "report_snapshot" });
  await saveAndConfirm("snapshot_append", "poker", "2026-10-04", [
    { kind: "roster", id: "batched", amount: "500" },
    { kind: "guest", id: "counterparty", name: "Sample Guest", amount: "-500" },
  ]);
  const fixed = await commandSuccess({
    operation: "player_report_at_snapshot",
    player: "batched",
    page: 1,
    size: 50,
    max_session_rowid: snapshot.max_session_rowid,
  });
  assert.equal(fixed.total, "1");
  assert.ok(!fixed.sessions.items.some(row => row.id === "snapshot_append"));

  const current = await commandSuccess({ operation: "player_report", player: "batched", page: 1, size: 50 });
  assert.equal(current.total, "501");
  assert.equal(current.sessions.items[0].id, "snapshot_append");
});

test("authorized Telegram reports show exact dates and confine guest details", async () => {
  const before = await database.prepare("SELECT COUNT(*) AS count FROM telegram_updates").first();
  const overall = await telegramSuccess("/leaderboard");
  assert.match(overall.text, /Alpha Player \[alpha\]: S\$7\.50/);
  assert.match(overall.text, /Idle Player \[idle\]: S\$0\.00/);
  assert.ok(!overall.text.includes("Sample Visitor"));

  const poker = await telegramSuccess("/gameboard poker");
  assert.match(poker.text, /Poker leaderboard/);
  assert.match(poker.text, /Beta Player \[beta\]: -S\$6\.00/);

  const player = await telegramSuccess("/stats alpha");
  assert.match(player.text, /Overall: S\$7\.50/);
  assert.match(player.text, /2026-10-03 — Poker \[session_one\]: S\$10\.00/);
  assert.ok(!player.text.includes("Sample Visitor"));

  const history = await telegramSuccess("/history");
  assert.match(history.text, /2026-10-03 — Poker \[session_one\]/);
  assert.ok(!history.text.includes("Sample Visitor"));

  const detail = await telegramSuccess("/details session_one");
  assert.match(detail.text, /Played: 2026-10-03 \(Asia\/Singapore\)/);
  assert.match(detail.text, /Sample Visitor \[visitor; one-time guest\]: -S\$4\.00/);
  const after = await database.prepare("SELECT COUNT(*) AS count FROM telegram_updates").first();
  assert.deepEqual(after, before, "read-only reports must not create workflow receipts");

  assert.equal((await telegram("/leaderboard", 999)).error, "Rejected");
  assert.equal((await telegram("/details session_one", 999)).error, "Rejected");
  assert.equal((await telegram("/leaderboard 10001")).error, "Limit");
});

test("widened positive and negative report totals survive ordering, RPCs and Telegram rendering", async () => {
  for (const id of ["wide_positive", "wide_negative"]) {
    await commandSuccess({ operation: "add_player", id, name: id });
  }
  for (const [suffix, amount] of [["large", "9223372036854775807"], ["extra", "2"]]) {
    await saveAndConfirm(`wide_${suffix}`, "poker", "0001-01-01", [
      { kind: "roster", id: "wide_positive", amount },
      { kind: "roster", id: "wide_negative", amount: (-BigInt(amount)).toString() },
    ]);
  }
  const total = "9223372036854775809";
  const overall = await commandSuccess({ operation: "leaderboard", game: null, page: 1, size: 50 });
  const poker = await commandSuccess({ operation: "leaderboard", game: "poker", page: 1, size: 50 });
  for (const entries of [overall.items, poker.page.items]) {
    assert.equal(entries[0].id, "wide_positive");
    assert.equal(entries[0].total, total);
    assert.equal(entries.at(-1).id, "wide_negative");
    assert.equal(entries.at(-1).total, `-${total}`);
  }
  for (const [id, prefix] of [["wide_positive", ""], ["wide_negative", "-"]]) {
    const report = await commandSuccess({ operation: "player_report", player: id, page: 1, size: 10 });
    assert.equal(report.total, `${prefix}${total}`);
    const reply = await telegramSuccess(`/stats ${id}`);
    assert.ok(reply.text.includes(`Overall: ${prefix}S$92233720368547758.09`));
  }
  for (const command of ["/leaderboard", "/gameboard poker"]) {
    const reply = await telegramSuccess(command);
    assert.match(reply.text, /wide_positive.*S\$92233720368547758\.09/);
    assert.match(reply.text, /wide_negative.*-S\$92233720368547758\.09/);
  }
  await saveAndConfirm("wide_cancel", "poker", "9999-12-31", [
    { kind: "roster", id: "wide_positive", amount: "-9223372036854775807" },
    { kind: "roster", id: "wide_negative", amount: "9223372036854775807" },
  ]);
  for (const [player, expected] of [["wide_positive", "2"], ["wide_negative", "-2"]]) {
    assert.equal((await commandSuccess({ operation: "player_report", player, page: 1, size: 10 })).total, expected);
  }
});
