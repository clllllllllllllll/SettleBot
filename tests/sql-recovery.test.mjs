import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, isAbsolute, join, relative, resolve } from "node:path";
import { promisify } from "node:util";
import { test } from "node:test";
import { unstable_dev } from "wrangler";

const execute = promisify(execFile);
const cli = resolve("node_modules/wrangler/bin/wrangler.js");
const created = "2026-11-01T00:00:00Z";
const tables = ["d1_migrations", "players", "games", "drafts", "draft_participants", "sessions",
  "session_entries", "processed_updates", "telegram_flows", "telegram_updates",
  "telegram_rate_events", "session_audits", "session_audit_entries"];

test("Wrangler local SQL export and isolated execute restore schema, operational state and application reads", async () => {
  const temporaryRoot = await realpath(tmpdir());
  const directory = await mkdtemp(join(temporaryRoot, "settlebot-sql-recovery-"));
  const [source, restored] = await Promise.all([configuration(directory, "source"), configuration(directory, "restored")]);
  let sourceWorker, restoredWorker;
  try {
    await wrangler(source, ["migrations", "apply", "LEDGER_DB"]);
    sourceWorker = await start(source);
    const sourceCall = caller(sourceWorker);
    const replay = await seed(sourceCall);
    const expected = await applicationReads(sourceCall);
    const resume = await sourceCall("telegram", { body: message(500, "/resume t100"), created_at: created, secret: "fake_secret" });
    await sourceWorker.stop(); sourceWorker = undefined;
    const originalDatabase = await databaseSnapshot(source);
    const backup = join(directory, "synthetic.sql");
    await wrangler(source, ["export", "LEDGER_DB", "--output", backup]);
    const sql = await readFile(backup);
    assert.ok(sql.toString().includes("session_audits"));
    const digest = createHash("sha256").update(sql).digest("hex");
    assert.match(digest, /^[a-f0-9]{64}$/);
    await wrangler(restored, ["execute", "LEDGER_DB", "--file", backup]);
    assert.deepEqual(await databaseSnapshot(restored), originalDatabase, "schema, migration metadata and all operational records restored");
    restoredWorker = await start(restored);
    const restoredCall = caller(restoredWorker);
    assert.deepEqual(await applicationReads(restoredCall), expected);
    assert.deepEqual(await restoredCall("telegram", { body: message(501, "/resume t100"), created_at: created, secret: "fake_secret" }), resume);
    assert.deepEqual(await restoredCall("telegram", replay.input), replay.result, "audited mutation receipt replays after restore");
    await restoredWorker.stop(); restoredWorker = undefined;
    assert.deepEqual(await databaseSnapshot(restored), originalDatabase, "reads and replay did not mutate restored records");
    console.log(`Local SQL recovery verified: ${tables.length} tables, SHA-256 ${digest}`);
  } finally {
    await sourceWorker?.stop(); await restoredWorker?.stop();
    await cleanup(directory, temporaryRoot);
  }
});

async function configuration(directory, target) {
  const folder = join(directory, target);
  await mkdir(folder);
  const bridge = join(folder, "worker.mjs");
  const rustWorker = resolve("build/d1-test/index.js").replaceAll("\\", "/");
  // Exists only in the generated temporary local fixture, never a deployable entry point.
  await writeFile(bridge, `import RustWorker from ${JSON.stringify(rustWorker)};
export default class extends RustWorker {
  async fetch(request) {
    const { kind, input } = await request.json();
    const value = kind === "telegram"
      ? await super.telegram_test_request(JSON.stringify(input), this.env)
      : await super.persistence_test_command(JSON.stringify(input), this.env);
    return new Response(value, { headers: { "content-type": "application/json" } });
  }
}`);
  const config = join(folder, "wrangler.json");
  await writeFile(config, JSON.stringify({ name: `settlebot-sql-${target}`, main: bridge,
    compatibility_date: "2026-10-13", // Pinned local workerd override only.
    d1_databases: [{ binding: "LEDGER_DB", database_name: `settlebot-sql-${target}`,
      database_id: `local-sql-${target}-only`, migrations_dir: resolve("migrations") }] }));
  return { config, folder };
}

async function wrangler(target, args) {
  const { stdout } = await execute(process.execPath, [cli, "d1", ...args, "--local", "--config", target.config], {
    env: { ...process.env, CI: "true", WRANGLER_SEND_METRICS: "false" }, timeout: 60_000, maxBuffer: 4 * 1024 * 1024,
    windowsHide: true,
  });
  return stdout;
}

async function start(target) {
  return unstable_dev(join(target.folder, "worker.mjs"), { config: target.config, local: true,
    persist: true, persistTo: join(target.folder, ".wrangler/state"), port: 0, inspectorPort: 0,
    inspect: false, logLevel: "none", experimental: { disableExperimentalWarning: true,
      disableDevRegistry: true, watch: false } });
}

function caller(worker) {
  return async (kind, input) => {
    const response = await worker.fetch("http://localhost/", { method: "POST", body: JSON.stringify({ kind, input }),
      signal: AbortSignal.timeout(10_000) });
    const result = await response.json();
    assert.ok(!result.error, result.error); return result.ok;
  };
}

async function databaseSnapshot(target) {
  const queries = ["SELECT type,name,tbl_name,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name",
    ...tables.map(table => `SELECT * FROM ${table} ORDER BY rowid`)];
  const output = await wrangler(target, ["execute", "LEDGER_DB", "--command", queries.join(";\n"), "--json"]);
  const results = JSON.parse(output);
  assert.equal(results.length, tables.length + 1);
  const schema = results[0].results;
  const records = {};
  for (const [index, table] of tables.entries()) records[table] = results[index + 1].results;
  for (const table of tables) assert.ok(records[table].length > 0, `representative ${table} records`);
  return { schema, records };
}

async function seed(call) {
  const command = input => call("command", input);
  for (const id of ["alpha", "beta"]) await command({ operation: "add_player", id, name: `Sample ${id}` });
  for (const id of ["original", "corrected", "voided"]) {
    await command({ operation: "save_draft", draft: { id, owner: 100, game: "poker", occurred_on: "2026-10-01", revision: 0,
      participants: [{ kind: "roster", id: "alpha", amount: "100" }, { kind: "roster", id: "beta", amount: "-80" },
        { kind: "guest", id: "visitor", name: "Sample Visitor", amount: "-20" }] } });
    await command({ operation: "confirm", id, owner: 100, revision: 0, update_key: `confirm_${id}`, created_at: created });
  }
  const telegram = (id, text, extra = {}) => call("telegram", { secret: "fake_secret", body: message(id, text), created_at: created, ...extra });
  const input = { secret: "fake_secret", body: message(101, "/correct corrected 0 +2.50 -2.00 -0.50 | Sample correction"), created_at: created };
  const result = await call("telegram", input);
  await telegram(102, "/correct voided 0 +3.50 -2.80 -0.70 | Before void");
  await telegram(103, "/void voided 1 | Sample void");
  await telegram(100, "/session", { rate: true });
  await telegram(100, "/session");
  return { input, result };
}

function message(update_id, text) {
  return { update_id, message: { date: 1, chat: { id: -100, type: "supergroup" }, from: { id: 100, is_bot: false }, text } };
}

async function applicationReads(call) {
  const command = input => call("command", input);
  const board = await command({ operation: "leaderboard", game: null, page: 1, size: 10 });
  assert.deepEqual(board.items.map(row => [row.id, row.total]), [["alpha", "350"], ["beta", "-280"]]);
  const details = [];
  for (const id of ["original", "corrected", "voided"]) details.push(await command({ operation: "details", id }));
  assert.equal(details[2].state, "voided");
  assert.deepEqual(details[2].entries.map(row => row.amount), ["350", "-280", "-70"]);
  assert.deepEqual(details[2].audits.map(row => row.revision), [1, 2]);
  return { board, details,
    game: await command({ operation: "leaderboard", game: "poker", page: 1, size: 10 }),
    player: await command({ operation: "player_report", player: "alpha", page: 1, size: 10 }),
    history: await command({ operation: "history", page: 1, size: 10 }) };
}

async function cleanup(directory, temporaryRoot) {
  const absolute = await realpath(directory);
  const child = relative(temporaryRoot, absolute);
  assert.ok(child && !child.startsWith("..") && !isAbsolute(child));
  assert.ok(basename(absolute).startsWith("settlebot-sql-recovery-"));
  await rm(absolute, { recursive: true, force: true });
}
