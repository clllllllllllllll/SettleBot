import assert from "node:assert/strict";
import { test } from "node:test";
import { createTestHarness } from "wrangler";

test("effective audit revisions, history, snapshots and export ignore physical insertion order", async () => {
  const servers = ["tests/wrangler.toml", "tests/wrangler-restore.toml"].map(configPath =>
    createTestHarness({ workers: [{ configPath }] }));
  try {
    const fixtures = [];
    for (const [index, server] of servers.entries()) {
      await server.listen();
      const worker = server.getWorker();
      const { LEDGER_DB: database } = await worker.getEnv();
      await worker.applyD1Migrations("LEDGER_DB");
      const api = await worker.getExport();
      const command = async input => {
        const result = JSON.parse(await api.command(JSON.stringify(input)));
        assert.ok(!result.error, result.error); return result.ok;
      };
      await command({ operation: "add_player", id: "alpha", name: "Alpha Player" });
      for (const id of ["corrected", "voided"]) {
        await save(command, id);
        const revisions = id === "corrected" ? [1, 2] : [1, 2, 3];
        for (const revision of index ? revisions.toReversed() : revisions) {
          await insertAudit(database, id, revision, revision === 3 ? "void" : "correction", revision * 10);
        }
      }
      fixtures.push({ database, command });
    }
    const read = async command => ({
      board: await command({ operation: "leaderboard", game: null, page: 1, size: 10 }),
      game: await command({ operation: "leaderboard", game: "poker", page: 1, size: 10 }),
      player: await command({ operation: "player_report", player: "alpha", page: 1, size: 10 }),
      history: await command({ operation: "history", page: 1, size: 10 }),
      corrected: await command({ operation: "details", id: "corrected" }),
      voided: await command({ operation: "details", id: "voided" }),
      export: await exports(command),
    });
    const [ordered, reversed] = await Promise.all(fixtures.map(fixture => read(fixture.command)));
    assert.deepEqual(reversed, ordered);
    assert.equal(ordered.board.items[0].total, "20");
    assert.deepEqual(ordered.history.items.map(row => row.id), ["corrected"]);
    assert.equal(ordered.voided.state, "voided");
    assert.deepEqual(ordered.voided.entries.map(entry => entry.amount), ["20", "-20"]);
    assert.deepEqual(ordered.voided.audits.map(audit => audit.revision), [1, 2, 3]);
    for (const fixture of fixtures) await verifySnapshot(fixture);
  } finally { for (const server of servers) await server.close(); }
});

async function verifySnapshot({ command, database }) {
  await save(command, "snapshot_case");
  await insertAudit(database, "snapshot_case", 2, "correction", 60);
  const snapshot = (await command({ operation: "report_snapshot" })).max_session_rowid;
  await insertAudit(database, "snapshot_case", 1, "correction", 50);
  for (const game of [null, "poker"]) {
    const report = await command({ operation: "leaderboard", game, page: 1, size: 10 });
    assert.equal((report.page ?? report).items[0].total, "80", "later physical older revision cannot win");
  }
  await insertAudit(database, "snapshot_case", 3, "correction", 70);
  for (const game of [null, "poker"]) {
    const old = await command({ operation: "leaderboard_at_snapshot", game, page: 1, size: 10, max_session_rowid: snapshot });
    assert.equal(old.items[0].total, "80");
    const live = await command({ operation: "leaderboard", game, page: 1, size: 10 });
    assert.equal((live.page ?? live).items[0].total, "90");
  }
  const oldPlayer = await command({ operation: "player_report_at_snapshot", player: "alpha", page: 1, size: 10, max_session_rowid: snapshot });
  assert.equal(oldPlayer.total, "80");
  assert.equal(oldPlayer.sessions.items.find(row => row.id === "snapshot_case").amount, "60");
}

async function save(command, id) {
  await command({ operation: "save_draft", draft: { id, owner: 100, game: "poker", occurred_on: "2026-10-01", revision: 0,
    participants: [{ kind: "roster", id: "alpha", amount: "10" },
      { kind: "guest", id: "visitor", name: "Sample Visitor", amount: "-10" }] } });
  await command({ operation: "confirm", id, owner: 100, revision: 0, update_key: `confirm_${id}`, created_at: "2026-11-01T00:00:00Z" });
}

async function insertAudit(database, session, revision, kind, amount) {
  const id = `${session}_${revision}`;
  const statements = [database.prepare(`INSERT INTO session_audits (id,session_id,revision,kind,actor_id,reason,created_at)
    VALUES (?,?,?,?,'100','Sample audit','2026-11-01T00:00:00Z')`).bind(id, session, revision, kind)];
  if (kind === "correction") {
    statements.push(database.prepare(`INSERT INTO session_audit_entries
      (audit_id,kind,identity_id,roster_player_id,guest_name,position,amount_cents)
      VALUES (?,'roster','alpha','alpha',NULL,0,?), (?,'guest','visitor',NULL,'Sample Visitor',1,?)`).bind(id, amount, id, -amount));
  }
  await database.batch(statements);
}

async function exports(command) {
  const sessions = []; let cursor;
  do {
    const page = await command({ operation: "export", cursor });
    sessions.push(...page.sessions); cursor = page.next_cursor;
  } while (cursor);
  return sessions;
}
