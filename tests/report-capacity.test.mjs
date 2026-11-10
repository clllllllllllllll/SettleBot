import assert from "node:assert/strict";
import { test } from "node:test";
import { createTestHarness } from "wrangler";

test("aggregate reports stop at the query budget and acknowledge capacity without partial totals", async () => {
  const server = createTestHarness({ workers: [{ configPath: "tests/wrangler.toml" }] });
  try {
    await server.listen();
    const worker = server.getWorker();
    const { LEDGER_DB: database } = await worker.getEnv();
    await worker.applyD1Migrations("LEDGER_DB");
    const api = await worker.getExport();
    const call = async input => JSON.parse(await api.command(JSON.stringify(input)));
    await database.prepare("INSERT INTO players VALUES ('history', 'History Player', 'active')").run();
    await seed(database, 0, 3698);
    await verifyReports(call, "3698");
    await seed(database, 3698, 3699);
    await verifyReports(call, "3699");
    await seed(database, 3699, 3700);
    for (const command of reports()) assert.equal((await call(command)).error, "ReportCapacity");
    for (const [index, text] of ["/leaderboard", "/gameboard poker", "/stats history"].entries()) {
      const body = { update_id: index + 1, message: { date: 1, chat: { id: -100, type: "supergroup" },
        from: { id: 100, is_bot: false }, text } };
      const result = JSON.parse(await api.telegram(JSON.stringify({ secret: "fake_secret", body,
        created_at: "2026-11-01T00:00:00Z", receive: true, delivery_http: true })));
      assert.equal(result.ok.status, 200);
      assert.match(result.ok.messages[0].text, /ledger-history capacity/);
      assert.ok(!result.ok.messages[0].text.includes("Shorten"));
      assert.ok(!result.ok.messages[0].text.includes("S$"), "no partial money results");
    }
    assert.ok((await call({ operation: "history", page: 1, size: 10 })).ok);
    assert.ok((await call({ operation: "details", id: "capacity_00000" })).ok);
  } finally { await server.close(); }
});

function reports() {
  return [
    { operation: "leaderboard", game: null, page: 1, size: 10 },
    { operation: "leaderboard", game: "poker", page: 1, size: 10 },
    { operation: "player_report", player: "history", page: 1, size: 10 },
  ];
}

async function verifyReports(call, total) {
  for (const command of reports()) {
    const result = await call(command);
    assert.ok(result.ok, result.error);
    const actual = command.operation === "player_report" ? result.ok.total
      : (result.ok.page ?? result.ok).items[0].total;
    assert.equal(actual, total);
  }
}

async function seed(database, start, end) {
  const numbers = `WITH RECURSIVE numbers(n) AS (SELECT ${start} UNION ALL SELECT n+1 FROM numbers WHERE n+1 < ${end})`;
  await database.batch([
    database.prepare(`${numbers} INSERT INTO drafts (id,owner_id,game_id,occurred_on,revision,state)
      SELECT printf('capacity_%05d',n),'100','poker','2026-01-01',1,'consumed' FROM numbers`),
    database.prepare(`${numbers} INSERT INTO sessions (id,owner_id,game_id,occurred_on,created_at)
      SELECT printf('capacity_%05d',n),'100','poker','2026-01-01','2026-11-01T00:00:00Z' FROM numbers`),
    database.prepare(`${numbers} INSERT INTO session_entries (session_id,kind,identity_id,roster_player_id,guest_name,position,amount_cents)
      SELECT printf('capacity_%05d',n),'roster','history','history',NULL,0,1 FROM numbers`),
    database.prepare(`${numbers} INSERT INTO session_entries (session_id,kind,identity_id,roster_player_id,guest_name,position,amount_cents)
      SELECT printf('capacity_%05d',n),'guest','counterparty',NULL,'Sample Guest',1,-1 FROM numbers`),
  ]);
}
