import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createTestHarness } from "wrangler";

const sourceServer = createTestHarness({ workers: [{ configPath: "tests/wrangler.toml" }] });
const restoreServer = createTestHarness({ workers: [{ configPath: "tests/wrangler-restore.toml" }] });
let sourceApi;
let restoreApi;
let sourceDatabase;
let restoreDatabase;
let sequence = 90000;
let preAuditSnapshot;
const created = "2026-11-03T12:00:00Z";

before(async () => {
  await sourceServer.listen();
  await restoreServer.listen();
  const source = sourceServer.getWorker();
  const restore = restoreServer.getWorker();
  ({ LEDGER_DB: sourceDatabase } = await source.getEnv());
  ({ LEDGER_DB: restoreDatabase } = await restore.getEnv());
  await source.applyD1Migrations("LEDGER_DB");
  await restore.applyD1Migrations("LEDGER_DB");
  sourceApi = await source.getExport();
  restoreApi = await restore.getExport();
  await seedSource();
  preAuditSnapshot = await success(sourceApi, { operation: "report_snapshot" });
});

after(async () => {
  await sourceServer.close();
  await restoreServer.close();
});

async function call(api, input) {
  return JSON.parse(await api.command(JSON.stringify(input)));
}

async function success(api, input) {
  const result = await call(api, input);
  assert.ok(!result.error, result.error);
  return result.ok;
}

async function seedSource() {
  for (const [id, name] of [["alpha", "Alpha Player"], ["beta", "Beta Player"]]) {
    await success(sourceApi, { operation: "add_player", id, name });
  }
  await saveAndConfirm("session_one", "2026-10-10", "1000", "-1000");
  await saveAndConfirm("session_two", "2026-10-11", "250", "-250");
}

async function saveAndConfirm(id, occurred_on, left, right) {
  await success(sourceApi, {
    operation: "save_draft",
    draft: {
      id, owner: 100, game: "poker", occurred_on, revision: 0,
      participants: [
        { kind: "roster", id: "alpha", amount: left },
        { kind: "roster", id: "beta", amount: right },
      ],
    },
  });
  await success(sourceApi, {
    operation: "confirm", id, owner: 100, revision: 0,
    update_key: `confirm_${id}`, created_at: created,
  });
}

function telegramMessage(text, owner = 100) {
  return {
    update_id: sequence++,
    message: {
      date: 1, message_id: sequence,
      chat: { id: -100, type: "supergroup" },
      from: { id: owner, is_bot: false }, text,
    },
  };
}

async function telegram(text, owner = 100) {
  return JSON.parse(await sourceApi.telegram(JSON.stringify({
    secret: "fake_secret", body: telegramMessage(text, owner), created_at: created,
  })));
}

test("authorized corrections and voids preserve originals and change current aggregates", async () => {
  assert.equal((await telegram("/correct session_one 0 +7.00 -7.00 | Corrected score", 999)).error, "Rejected");
  assert.equal((await sourceDatabase.prepare("SELECT COUNT(*) AS count FROM session_audits").first()).count, 0);
  assert.equal((await telegram("/correct session_one 0 +7.00 -6.00 | Does not balance")).error, "Invalid");
  assert.equal((await telegram("/correct session_one 0 +7.00 | Missing participant")).error, "Invalid");
  assert.equal((await telegram("/void session_two 0 | ")).error, "AuditSyntax");
  assert.equal((await sourceDatabase.prepare("SELECT COUNT(*) AS count FROM session_audits").first()).count, 0);

  await sourceDatabase.prepare(`CREATE TRIGGER fail_audit_receipt BEFORE INSERT ON telegram_updates
    WHEN NEW.response LIKE '%Audited correction%' BEGIN SELECT RAISE(ABORT, 'injected audit receipt failure'); END`).run();
  assert.equal((await telegram("/correct session_one 0 +7.00 -7.00 | Corrected score")).error, "Storage");
  assert.equal((await sourceDatabase.prepare("SELECT COUNT(*) AS count FROM session_audits").first()).count, 0);
  await sourceDatabase.prepare("DROP TRIGGER fail_audit_receipt").run();

  const correction = await telegram("/correct session_one 0 +7.00 -7.00 | Corrected score");
  assert.match(correction.ok.text, /Audited correction saved/);
  const voided = await telegram("/void session_two 0 | Duplicate entry");
  assert.match(voided.ok.text, /Audited void saved/);

  const original = await sourceDatabase.prepare(
    "SELECT CAST(amount_cents AS TEXT) AS amount FROM session_entries WHERE session_id = 'session_one' ORDER BY position",
  ).all();
  assert.deepEqual(original.results.map(row => row.amount), ["1000", "-1000"]);

  const board = await success(sourceApi, { operation: "leaderboard", game: null, page: 1, size: 2 });
  assert.deepEqual(board.items.map(row => [row.id, row.total]), [["alpha", "700"], ["beta", "-700"]]);
  const beforeAudit = await success(sourceApi, {
    operation: "player_report_at_snapshot", player: "alpha", page: 1, size: 10,
    max_session_rowid: preAuditSnapshot.max_session_rowid,
  });
  assert.equal(beforeAudit.total, "1250", "a captured report excludes later corrections and voids");
  const history = await success(sourceApi, { operation: "history", page: 1, size: 10 });
  assert.deepEqual(history.items.map(row => row.id), ["session_one"]);

  const corrected = await success(sourceApi, { operation: "details", id: "session_one" });
  assert.equal(corrected.state, "corrected");
  assert.deepEqual(corrected.entries.map(row => row.amount), ["700", "-700"]);
  assert.deepEqual(corrected.audits.map(row => [row.revision, row.kind, row.reason]), [
    [1, "correction", "Corrected score"],
  ]);
  const duplicate = await success(sourceApi, { operation: "details", id: "session_two" });
  assert.equal(duplicate.state, "voided");
  assert.equal(duplicate.audits[0].kind, "void");

  const longReason = "R".repeat(256);
  for (let revision = 2; revision <= 10; revision += 1) {
    const amounts = revision === 10 ? ["600", "-600"] : ["700", "-700"];
    await success(sourceApi, {
      operation: "audit", key: `correction_${revision}`, id: "session_one", actor: 100,
      expected_revision: revision - 1,
      kind: "correction", reason: longReason, created_at: created, amounts,
    });
  }
  const firstPage = await success(sourceApi, { operation: "details", id: "session_one" });
  assert.deepEqual(firstPage.entries.map(row => row.amount), ["600", "-600"]);
  assert.equal(firstPage.audits.length, 3);
  assert.equal(firstPage.next_after, 3);
  const telegramFirstPage = await telegram("/details session_one");
  assert.match(telegramFirstPage.ok.text, /Next: \/details session_one 3/);
  const telegramPage = await telegram("/details session_one 7");
  assert.ok(telegramPage.ok.text.length <= 4096);
  assert.match(telegramPage.ok.text, /Revision 10/);

  await seedHighRowidSession();
});

test("portable JSON backup restores ledger totals and audit lineage into an isolated D1", async () => {
  assert.equal((await exportHttp()).status, 403);
  assert.equal((await exportHttp("wrong-secret")).status, 403);
  assert.equal((await exportHttp("fake_export_secret", "invalid")).status, 400);
  await seedHistoricalRoster();
  const expectedBoard = await success(sourceApi, {
    operation: "leaderboard", game: null, page: 1, size: 2,
  });
  const pages = [];
  let cursor;
  for (;;) {
    const response = await exportHttp("fake_export_secret", cursor);
    assert.equal(response.status, 200);
    assert.equal(response.cache_control, "no-store");
    const exported = JSON.parse(response.body);
    assert.equal(exported.format, "settlebot-ledger-v1");
    assert.ok(exported.players.length <= 24);
    assert.ok(exported.games.length <= 24);
    assert.ok(exported.sessions.every(session => session.audits.length <= 8));
    pages.push(exported);
    if (pages.length === 1) await appendCatalogsAfterSnapshot();
    cursor = exported.next_cursor;
    if (!cursor) break;
  }
  assert.equal(pages.length, 5, "catalog and audit history each span bounded chunks");
  const exportedPlayers = pages.flatMap(page => page.players);
  const exportedGames = pages.flatMap(page => page.games);
  assert.equal(exportedPlayers.length, 25);
  assert.equal(new Set(exportedPlayers.map(player => player.id)).size, 25);
  assert.equal(exportedPlayers.filter(player => player.status === "active").length, 24);
  assert.ok(exportedPlayers.some(player => player.id === "inactive_history"));
  assert.ok(!exportedPlayers.some(player => player.id === "late_player"));
  assert.equal(exportedGames.length, 2);
  assert.equal(new Set(exportedGames.map(game => game.id)).size, 2);
  assert.ok(!exportedGames.some(game => game.id === "late_game"));
  assert.ok(pages.some(page => page.sessions[0]?.id === "session_high"));
  await restorePages(pages);

  assert.deepEqual(await ledgerCounts(restoreDatabase), await ledgerCounts(sourceDatabase));
  const restoredBoard = await success(restoreApi, { operation: "leaderboard", game: null, page: 1, size: 2 });
  assert.deepEqual(restoredBoard, expectedBoard);
  const restoredDetail = await success(restoreApi, { operation: "details", id: "session_one" });
  assert.equal(restoredDetail.state, "corrected");
  assert.deepEqual(restoredDetail.entries.map(row => row.amount), ["600", "-600"]);
  const restoredAudits = await allAuditPages(restoreApi, "session_one");
  assert.equal(restoredAudits.length, 10);
  assert.deepEqual(restoredAudits[0], {
    revision: 1, kind: "correction", actor: "100",
    reason: "Corrected score", created_at: created,
  });
});

test("void is terminal, audit revisions are explicit, and exact successful updates replay", async () => {
  await saveAndConfirm("terminal", "2026-10-13", "500", "-500");
  const send = async body => JSON.parse(await sourceApi.telegram(JSON.stringify({
    secret: "fake_secret", body, created_at: created,
  })));
  const correction = telegramMessage("/correct terminal 0 +3.00 -3.00 | Sample correction");
  const [corrected, replayed] = await Promise.all([send(correction), send(correction)]);
  assert.deepEqual(replayed, corrected, "same-key concurrent audit replays one receipt");
  assert.ok(corrected.ok, corrected.error);
  const voidCommand = telegramMessage("/void terminal 1 | Sample void");
  const voided = await send(voidCommand);
  assert.ok(voided.ok, voided.error);
  const detail = await success(sourceApi, { operation: "details", id: "terminal" });
  assert.equal(detail.state, "voided"); assert.equal(detail.revision, "2");
  assert.deepEqual(detail.entries.map(entry => entry.amount), ["300", "-300"]);
  const count = await auditCounts("terminal");
  for (const text of ["/correct terminal 2 +1.00 -1.00 | No restore", "/void terminal 2 | No second void",
    "/correct session_one 0 +1.00 -1.00 | Stale", "/void session_one 0 | Stale"]) {
    assert.equal((await telegram(text)).error, "AuditStale");
  }
  assert.deepEqual(await send(correction), corrected, "older successful command still replays");
  assert.deepEqual(await send(voidCommand), voided);
  assert.deepEqual(await auditCounts("terminal"), count);
  assert.match((await telegram("/details terminal")).ok.text, /Current revision: 2/);
  const oldSyntax = JSON.parse(await sourceApi.telegram(JSON.stringify({ secret: "fake_secret",
    body: telegramMessage("/void terminal | Old syntax"), created_at: created, deliver: true })));
  assert.match(oldSyntax.ok.messages[0].text, /SESSION_ID REVISION/);
});

test("prepared audit races enforce expected revision inside the transaction including entryless voids", async () => {
  for (const [label, leftKind, rightKind] of [["cc", "correction", "correction"],
    ["cv", "correction", "void"], ["vv", "void", "void"]]) {
    const id = `race_${label}`;
    await saveAndConfirm(id, "2026-10-14", "500", "-500");
    const input = (side, kind) => ({ key: `race_${label}_${side}`, id, expected_revision: 0, actor: 100,
      kind, reason: `Sample ${side}`, created_at: created, amounts: kind === "correction" ? ["200", "-200"] : [] });
    const results = await success(sourceApi, { operation: "audit_race", left: input("left", leftKind), right: input("right", rightKind) });
    assert.deepEqual(results.toSorted(), ["Err(AuditStale)", "Ok(())"]);
    const counts = await auditCounts(id);
    assert.equal(counts.audits, 1); assert.equal(counts.receipts, 1);
    const detail = await success(sourceApi, { operation: "details", id });
    assert.equal(detail.revision, "1");
    assert.equal(counts.entries, detail.state === "voided" ? 0 : 2);
    const original = await sourceDatabase.prepare("SELECT CAST(amount_cents AS TEXT) AS amount FROM session_entries WHERE session_id = ? ORDER BY position").bind(id).all();
    assert.deepEqual(original.results.map(row => row.amount), ["500", "-500"]);
    assert.deepEqual(detail.entries.map(row => row.id), ["alpha", "beta"]);
    assert.equal(detail.audits[0].actor, "100");
  }
});

test("void receipt storage failure rolls back the audit header and remains distinct from stale state", async () => {
  await saveAndConfirm("void_rollback", "2026-10-15", "100", "-100");
  await sourceDatabase.prepare(`CREATE TRIGGER fail_void_receipt BEFORE INSERT ON telegram_updates
    WHEN NEW.response LIKE '%Audited void%' BEGIN SELECT RAISE(ABORT, 'injected void receipt failure'); END`).run();
  assert.equal((await telegram("/void void_rollback 0 | Sample void")).error, "Storage");
  assert.equal((await auditCounts("void_rollback")).audits, 0);
  await sourceDatabase.prepare("DROP TRIGGER fail_void_receipt").run();
  assert.ok((await telegram("/void void_rollback 0 | Sample void")).ok);
});

test("legacy successful audit receipts replay after syntax changes without permitting new unversioned actions", async () => {
  await saveAndConfirm("legacy", "2026-10-16", "500", "-500");
  const recorded = [];
  for (const [revision, kind] of ["correction", "void"].entries()) {
    const text = kind === "correction" ? "/correct legacy +3.00 -3.00 | Legacy action" : "/void legacy | Legacy action";
    const body = telegramMessage(text);
    const key = `telegram_${body.update_id}`;
    await success(sourceApi, { operation: "audit", key, id: "legacy", expected_revision: revision,
      actor: 100, kind, reason: "Legacy action", created_at: created, amounts: kind === "correction" ? ["300", "-300"] : [] });
    const reply = { method: "sendMessage", chat_id: -100, text: `Audited ${kind} saved for session legacy.`,
      reply_markup: { inline_keyboard: [] } };
    const fingerprint = JSON.stringify(["-100", "100", false, text]);
    await sourceDatabase.prepare("INSERT INTO telegram_updates (update_key,fingerprint,response) VALUES (?,?,?)")
      .bind(key, fingerprint, JSON.stringify(reply)).run();
    recorded.push({ body, reply });
  }
  const before = await auditCounts("legacy");
  for (const { body, reply } of recorded) {
    const result = JSON.parse(await sourceApi.telegram(JSON.stringify({ secret: "fake_secret", body, created_at: created })));
    assert.deepEqual(result.ok, reply);
  }
  assert.equal((await telegram("/void legacy | New unversioned action")).error, "AuditSyntax");
  const forged = structuredClone(recorded[0].body); forged.message.text += " changed";
  const rejected = JSON.parse(await sourceApi.telegram(JSON.stringify({ secret: "fake_secret", body: forged, created_at: created })));
  assert.equal(rejected.error, "Rejected");
  assert.deepEqual(await auditCounts("legacy"), before);
});

async function auditCounts(id) {
  return sourceDatabase.prepare(`SELECT
    (SELECT COUNT(*) FROM session_audits WHERE session_id = ?1) AS audits,
    (SELECT COUNT(*) FROM session_audit_entries e JOIN session_audits a ON a.id = e.audit_id WHERE a.session_id = ?1) AS entries,
    (SELECT COUNT(*) FROM telegram_updates t JOIN session_audits a ON a.id = t.update_key WHERE a.session_id = ?1) AS receipts`)
    .bind(id).first();
}

async function exportHttp(token, cursor) {
  const result = JSON.parse(await sourceApi.telegram(JSON.stringify({
    export_http: true, export_token: token, cursor,
    secret: null, body: {}, created_at: created,
  })));
  assert.ok(!result.error, result.error);
  return result.ok;
}

async function allAuditPages(api, id) {
  const audits = [];
  let after_revision;
  for (;;) {
    const detail = await success(api, { operation: "details", id, after_revision });
    audits.push(...detail.audits);
    after_revision = detail.next_after;
    if (!after_revision) return audits;
  }
}

async function restorePages(pages) {
  for (const player of pages.flatMap(page => page.players)) {
    await restoreDatabase.prepare(
      "INSERT INTO players (id, display_name, status) VALUES (?, ?, ?) ON CONFLICT(id) DO NOTHING",
    ).bind(player.id, player.display_name, player.status).run();
  }
  for (const game of pages.flatMap(page => page.games)) {
    await restoreDatabase.prepare(
      "INSERT INTO games (id, display_name) VALUES (?, ?) ON CONFLICT(id) DO NOTHING",
    ).bind(game.id, game.display_name).run();
  }
  for (const session of pages.flatMap(page => page.sessions)) await restoreSession(session);
}

async function restoreSession(session) {
  if (session.entries.length === 0) {
    return restoreAudits(session);
  }
  await restoreDatabase.prepare(
    "INSERT INTO drafts (id, owner_id, game_id, occurred_on, revision, state) VALUES (?, ?, ?, ?, 1, 'consumed')",
  ).bind(session.id, session.owner_id, session.game_id, session.occurred_on).run();
  await restoreDatabase.prepare(
    "INSERT INTO sessions (id, owner_id, game_id, occurred_on, created_at) VALUES (?, ?, ?, ?, ?)",
  ).bind(session.id, session.owner_id, session.game_id, session.occurred_on, session.created_at).run();
  for (const entry of session.entries) await restoreEntry("session_entries", "session_id", session.id, entry);
  await restoreAudits(session);
}

async function restoreAudits(session) {
  for (const audit of session.audits) {
    await restoreDatabase.prepare(
      "INSERT INTO session_audits (id, session_id, revision, kind, actor_id, reason, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    ).bind(audit.id, session.id, audit.revision, audit.kind, audit.actor_id, audit.reason, audit.created_at).run();
    for (const entry of audit.entries) await restoreEntry("session_audit_entries", "audit_id", audit.id, entry);
  }
}

async function seedHighRowidSession() {
  await sourceDatabase.prepare(
    "INSERT INTO drafts (id, owner_id, game_id, occurred_on, revision, state) VALUES ('session_high', '100', 'poker', '2026-10-12', 1, 'consumed')",
  ).run();
  await sourceDatabase.prepare(
    `INSERT INTO sessions (rowid, id, owner_id, game_id, occurred_on, created_at)
     VALUES (10001, 'session_high', '100', 'poker', '2026-10-12', ?)`,
  ).bind(created).run();
  for (const [position, player] of [[0, "alpha"], [1, "beta"]]) {
    await sourceDatabase.prepare(
      `INSERT INTO session_entries
       (session_id, kind, identity_id, roster_player_id, guest_name, position, amount_cents)
       VALUES ('session_high', 'roster', ?, ?, NULL, ?, 0)`,
    ).bind(player, player, position).run();
  }
}

async function appendCatalogsAfterSnapshot() {
  await sourceDatabase.prepare(
    "INSERT INTO players (id, display_name, status) VALUES ('late_player', 'Late Player', 'inactive')",
  ).run();
  await sourceDatabase.prepare(
    "INSERT INTO games (id, display_name) VALUES ('late_game', 'Late Game')",
  ).run();
  assert.equal((await sourceDatabase.prepare(
    "SELECT COUNT(*) AS count FROM players WHERE id = 'late_player'",
  ).first()).count, 1);
  assert.equal((await sourceDatabase.prepare(
    "SELECT COUNT(*) AS count FROM games WHERE id = 'late_game'",
  ).first()).count, 1);
}

async function seedHistoricalRoster() {
  for (let number = 1; number <= 22; number += 1) {
    const id = `active_${number.toString().padStart(2, "0")}`;
    await sourceDatabase.prepare(
      "INSERT INTO players (id, display_name, status) VALUES (?, ?, 'active')",
    ).bind(id, `Active ${number}`).run();
  }
  await sourceDatabase.prepare(
    "INSERT INTO players (id, display_name, status) VALUES ('inactive_history', 'Inactive History', 'inactive')",
  ).run();
}

async function restoreEntry(table, ownerColumn, owner, entry) {
  await restoreDatabase.prepare(
    `INSERT INTO ${table} (${ownerColumn}, kind, identity_id, roster_player_id, guest_name, position, amount_cents)
     VALUES (?, ?, ?, ?, ?, ?, CAST(? AS INTEGER))`,
  ).bind(owner, entry.kind, entry.identity_id, entry.roster_player_id, entry.guest_name,
    entry.position, entry.amount_cents).run();
}

async function ledgerCounts(database) {
  return database.prepare(`SELECT
    (SELECT COUNT(*) FROM sessions) AS sessions,
    (SELECT COUNT(*) FROM session_entries) AS entries,
    (SELECT COUNT(*) FROM session_audits) AS audits,
    (SELECT COUNT(*) FROM session_audit_entries) AS audit_entries`).first();
}
