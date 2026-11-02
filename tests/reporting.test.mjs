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
