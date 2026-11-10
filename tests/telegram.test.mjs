import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createTestHarness } from "wrangler";

const server = createTestHarness({ workers: [{ configPath: "tests/wrangler.toml" }] });
let api, database;
let sequence = 10000;
const created = "2026-11-01T16:00:00Z";
const tables = ["players", "games", "drafts", "draft_participants", "sessions", "session_entries",
  "processed_updates", "telegram_flows", "telegram_updates", "telegram_rate_events"];

before(async () => {
  await server.listen();
  const worker = server.getWorker();
  ({ LEDGER_DB: database } = await worker.getEnv());
  await worker.applyD1Migrations("LEDGER_DB");
  api = await worker.getExport();
});
after(async () => { await server.close(); });

function message(text, owner = 100) {
  return { update_id: sequence++, message: { date: 1, message_id: sequence,
    chat: { id: -100, type: "supergroup" }, from: { id: owner, is_bot: false }, text } };
}
function callback(data, owner = 100) {
  return { update_id: sequence++, callback_query: { id: `callback_${sequence}`, from: { id: owner, is_bot: false },
    message: { date: 1, message_id: sequence, chat: { id: -100, type: "supergroup" }, from: { id: 500, is_bot: true } }, data } };
}
async function request(body, options = {}) {
  return JSON.parse(await api.telegram(JSON.stringify({ secret: "fake_secret", body, created_at: created, ...options })));
}
async function success(body, options) {
  const result = await request(body, options);
  assert.ok(!result.error, result.error);
  return result.ok;
}
async function snapshot() {
  const results = await database.batch(tables.map(table => database.prepare(`SELECT * FROM ${table} ORDER BY rowid`)));
  return results.map(result => result.results);
}
async function rejected(body, options = {}) {
  const before = await snapshot();
  const result = await request(body, options);
  assert.ok(result.error, "request must fail");
  assert.deepEqual(await snapshot(), before, "rejected input performed no writes");
  return result.error;
}
function buttons(reply) { return reply.reply_markup.inline_keyboard.flat(); }
function button(reply, label) {
  const found = buttons(reply).find(item => item.text.includes(label));
  assert.ok(found, `Missing ${label} in ${JSON.stringify(reply)}`);
  return found.callback_data;
}
function step(reply, value, guest = false) {
  const command = guest ? "guest" : "step";
  const match = reply.text.match(new RegExp(`/${command} (t\\d+) (\\d+)`));
  assert.ok(match, reply.text);
  return message(`/${command} ${match[1]} ${match[2]} ${value}`);
}
function current(reply, action) {
  const first = buttons(reply)[0].callback_data.split(":");
  return `${first[0]}:${first[1]}:${action}`;
}
async function selectParticipants() {
  let reply = await success(message("/session"));
  const id = reply.text.match(/Session (t\d+)/)[1];
  reply = await success(callback(button(reply, "Poker")));
  reply = await success(step(reply, "2026-10-31"));
  reply = await success(callback(button(reply, "Sample Player")));
  reply = await success(step(reply, "Sample Visitor", true));
  return { id, reply };
}
async function review(amounts = ["+12.50", "-12.50"]) {
  let { id, reply } = await selectParticipants();
  reply = await success(callback(button(reply, "Enter amounts")));
  for (const amount of amounts) reply = await success(step(reply, amount));
  return { id, reply };
}

test("transport rejects secrets, chats, anonymous senders and non-allowlisted administrators without writes", async () => {
  for (const secret of [null, "", "incorrect"]) await rejected(message("/player Forbidden"), { secret });
  for (const text of ["/session", "/player Forbidden", "/game Forbidden", "/step t1 0 +1.00", "/guest t1 0 Guest"]) {
    for (const type of ["private", "channel"]) {
      const body = message(text); body.message.chat.type = type; await rejected(body);
    }
    const other = message(text); other.message.chat.id = -200; await rejected(other);
    const anonymous = message(text); anonymous.message.sender_chat = { id: -100 }; await rejected(anonymous);
    const missing = message(text); delete missing.message.from; await rejected(missing);
    const admin = message(text, 999); Object.assign(admin.message.from, { is_admin: true, status: "creator", username: "owner" });
    await rejected(admin);
  }
  for (const action of ["g:0", "p:0", "done", "edit", "confirm"]) {
    await rejected(callback(`t1:0:${action}`, 999));
    const other = callback(`t1:0:${action}`); other.callback_query.message.chat.id = -200; await rejected(other);
    const missing = callback(`t1:0:${action}`); delete missing.callback_query.message; await rejected(missing);
  }
});

test("real Worker HTTP adapter acknowledges permanent rejections without writes or Telegram sends", async () => {
  const original = await snapshot();
  for (const secret of [null, "incorrect"]) {
    const result = await success({ malformed: true }, { http: true, secret });
    assert.equal(result.status, 403, "secret checked before parsing update");
  }
  const nonAdministrator = message("/player Forbidden", 999);
  const otherGroup = message("/player Forbidden"); otherGroup.message.chat.id = -200;
  const privateChat = message("/player Forbidden"); privateChat.message.chat.type = "private";
  const channel = message("/player Forbidden"); channel.message.chat.type = "channel";
  const anonymous = message("/player Forbidden"); anonymous.message.sender_chat = { id: -100 };
  const missingSender = message("/player Forbidden"); delete missingSender.message.from;
  const unsupported = { update_id: sequence++, channel_post: message("/player Forbidden").message };
  const otherBot = message("/player@OtherBot Forbidden");
  for (const body of [nonAdministrator, otherGroup, privateChat, channel, anonymous, missingSender, unsupported, otherBot]) {
    assert.equal((await success(body, { http: true })).status, 200);
  }
  assert.deepEqual(await snapshot(), original);
});

test("oversized webhook, text, callback, and display-name inputs are bounded without ledger writes", async () => {
  await rejected(message("x".repeat(513)));
  await rejected(message(`/player ${"n".repeat(65)}`));
  await rejected(callback("x".repeat(65)));
  const before = await snapshot();
  const oversized = message("x".repeat(17 * 1024));
  assert.equal((await success(oversized, { http: true })).status, 200);
  assert.deepEqual(await snapshot(), before);
});

test("production admission and delivery ignore ordinary text even with an open draft", async () => {
  for (const text of ["ordinary group conversation", "hello@OtherBot"]) {
    const before = await snapshot();
    const result = await success(message(text), { receive: true });
    assert.deepEqual(result, { response: { ok: true }, messages: [] });
    assert.deepEqual(await snapshot(), before);
  }
  const draft = await success(message("/session@SETTLEBOT"), { receive: true });
  assert.match(draft.messages[0].text, /Session/);
  const before = await snapshot();
  assert.deepEqual((await success(message("another conversation"), { receive: true })).messages, []);
  assert.deepEqual(await snapshot(), before, "open draft is untouched");
  const invalid = message("/step");
  const result = await success(invalid, { receive: true });
  assert.equal(result.messages.length, 1, "malformed recognized command gets guidance");
  assert.ok(await database.prepare("SELECT 1 FROM telegram_rate_events WHERE update_key = ?")
    .bind(`telegram_${invalid.update_id}`).first(), "malformed command consumed admission");
  const other = await snapshot();
  assert.equal((await request(message("/session@OtherBot"), { receive: true })).error, "Rejected");
  assert.equal((await request(message("plain text"), { receive: true, secret: "incorrect" })).error, "Unauthorized");
  assert.deepEqual(await snapshot(), other);
});

test("per-user throttling is idempotent by update key and recovers after its rolling window", async () => {
  const replay = message("/session");
  await success(replay, { rate: true });
  await success(replay);
  const later = "2026-11-01T16:01:01Z";
  for (let index = 0; index < 20; index += 1) {
    await success(message("/session"), { rate: true, created_at: later });
  }
  assert.equal((await request(message("/session"), { rate: true, created_at: later })).error, "RateLimited");
  await success(replay, { rate: true, created_at: later });
  await success(message("/session", 101), { rate: true, created_at: later });
});

test("catalog creation uses numeric authorization and atomically replays update IDs", async () => {
  const player = message("/player@SettleBot Sample Player");
  const replies = await Promise.all([success(player), success(player)]);
  assert.deepEqual(replies[0], replies[1]);
  const game = message("/game@settlebot Sample Game");
  assert.deepEqual(await success(game), await success(game));
  assert.equal((await database.prepare("SELECT COUNT(*) AS n FROM players").first()).n, 1);
  assert.equal((await database.prepare("SELECT COUNT(*) AS n FROM games").first()).n, 3);
  const forged = structuredClone(player); forged.message.text = "/game Changed";
  assert.equal(await rejected(forged), "Rejected");
  const another = structuredClone(player); another.message.from.id = 101;
  assert.equal(await rejected(another), "Rejected");
});

test("guided flow reviews Singapore date and exact total, saves guests, and safely replays confirmation", async () => {
  const { id, reply } = await review();
  assert.match(reply.text, /Played: 2026-10-31 \(Asia\/Singapore\)/);
  assert.match(reply.text, /Sample Visitor \(one-time guest\): -S\$12.50/);
  assert.match(reply.text, /Total: S\$0.00/);
  const update = callback(button(reply, "Confirm session"));
  const saved = await success(update);
  assert.deepEqual(await success(update, { created_at: "2027-01-01T00:00:00Z" }), saved);
  const session = await database.prepare("SELECT * FROM sessions WHERE id = ?").bind(id).first();
  assert.equal(session.occurred_on, "2026-10-31"); assert.equal(session.created_at, created);
  assert.equal((await database.prepare("SELECT COUNT(*) AS n FROM session_entries WHERE session_id = ?").bind(id).first()).n, 2);
  assert.equal((await database.prepare("SELECT COUNT(*) AS n FROM processed_updates WHERE session_id = ?").bind(id).first()).n, 1);
  assert.equal((await database.prepare("SELECT COUNT(*) AS n FROM players").first()).n, 1);
  await rejected(callback(button(reply, "Confirm session")));
  assert.match((await success(message(`/resume ${id}`))).text, /Saved/);
  let next = await success(message("/session"));
  next = await success(callback(button(next, "Poker")));
  next = await success(step(next, "2026-11-02"));
  assert.ok(!JSON.stringify(next).includes("Sample Visitor"), "guests never enter other pickers");
});

test("forged, stale, cross-owner and wrong-state callbacks and text steps cause no writes", async () => {
  const start = message("/session");
  const reply = await success(start);
  const game = button(reply, "Poker");
  await rejected(callback(game, 101));
  await rejected(callback(current(reply, "confirm")));
  await rejected(callback(current(reply, "g:9999")));
  await rejected(callback(game.replace(":0:", ":99:")));
  const date = await success(callback(game));
  await rejected(callback(game));
  await rejected(step(date, "2026-02-30"));
  const foreign = step(date, "2026-11-02"); foreign.message.from.id = 101; await rejected(foreign);
  const resumed = await success(message(`/resume t${start.update_id}`));
  assert.deepEqual(resumed, date);
  await rejected(message(`/resume t${start.update_id}`, 101));
});

test("nonzero reviews cannot be forged into confirmation and administrators can re-enter amounts", async () => {
  let { reply } = await review(["+12.50", "-10.00"]);
  assert.match(reply.text, /Total: S\$2.50/);
  assert.ok(!buttons(reply).some(button => button.text === "Confirm session"));
  await rejected(callback(current(reply, "confirm")));
  reply = await success(callback(button(reply, "Re-enter amounts")));
  await rejected(step(reply, "+1.001"));
  reply = await success(step(reply, "+0.01"));
  reply = await success(step(reply, "-0.01"));
  assert.match((await success(callback(button(reply, "Confirm session")))).text, /Saved/);
});

test("concurrent duplicate steps replay one response and competing revisions cannot overwrite each other", async () => {
  const start = message("/session");
  const reply = await success(start);
  const first = callback(button(reply, "Poker"));
  assert.deepEqual(...await Promise.all([success(first), success(first)]));
  const date = await success(first);
  const alternatives = await Promise.all([request(step(date, "2026-10-30")), request(step(date, "2026-10-31"))]);
  assert.equal(alternatives.filter(result => result.ok).length, 1);
  assert.equal(alternatives.filter(result => result.error).length, 1);
  assert.deepEqual(await success(message(`/resume t${start.update_id}`)), alternatives.find(result => result.ok).ok);
});

test("final receipt failure rolls back confirmation, entries, draft, workflow and both idempotency records", async () => {
  const { reply } = await review();
  const update = callback(button(reply, "Confirm session"));
  await database.prepare(`CREATE TRIGGER fail_telegram_receipt BEFORE INSERT ON telegram_updates
    WHEN NEW.update_key = 'telegram_${update.update_id}' BEGIN SELECT RAISE(ABORT, 'injected failure'); END`).run();
  await rejected(update);
  await database.prepare("DROP TRIGGER fail_telegram_receipt").run();
  assert.match((await success(update)).text, /Saved/);
});

test("catalog and guided steps roll back on failed receipt and remain retryable", async () => {
  const flow = await success(message("/session"));
  for (const update of [message("/player Rollback Sample"), message("/game Rollback Game"), message("/session"), callback(button(flow, "Poker"))]) {
    await database.prepare(`CREATE TRIGGER fail_telegram_edit BEFORE INSERT ON telegram_updates
      WHEN NEW.update_key = 'telegram_${update.update_id}' BEGIN SELECT RAISE(ABORT, 'injected failure'); END`).run();
    await rejected(update);
    await database.prepare("DROP TRIGGER fail_telegram_edit").run();
    await success(update);
  }
});

test("same confirmation update racing replays one receipt and preserves full cent precision", async () => {
  const { id, reply } = await review(["+92233720368547758.07", "-92233720368547758.07"]);
  assert.match(reply.text, /Total: S\$0.00/);
  const update = callback(button(reply, "Confirm session"));
  const replies = await Promise.all([success(update), success(update)]);
  assert.deepEqual(replies[0], replies[1]);
  const entries = await database.prepare("SELECT CAST(amount_cents AS TEXT) AS cents FROM session_entries WHERE session_id = ? ORDER BY position").bind(id).all();
  assert.deepEqual(entries.results.map(row => row.cents), ["9223372036854775807", "-9223372036854775807"]);
});

test("inactive players are excluded from ordinary participant pickers", async () => {
  await database.prepare("INSERT INTO players (id, display_name, status) VALUES ('inactive_sample', 'Inactive Sample', 'inactive')").run();
  let reply = await success(message("/session"));
  reply = await success(callback(button(reply, "Poker")));
  reply = await success(step(reply, "2026-11-02"));
  assert.ok(!JSON.stringify(reply).includes("Inactive Sample"));
});

test("corrupted workflow ownership and values fail closed without writes", async () => {
  const start = message("/session");
  const reply = await success(start);
  const id = `t${start.update_id}`;
  const original = await database.prepare("SELECT document FROM telegram_flows WHERE id = ?").bind(id).first();
  const changed = JSON.parse(original.document); changed.owner = 101;
  await database.prepare("UPDATE telegram_flows SET document = ? WHERE id = ?").bind(JSON.stringify(changed), id).run();
  assert.equal(await rejected(callback(button(reply, "Poker"))), "Storage");
  changed.owner = 100; changed.games[0].id = "bad/id";
  await database.prepare("UPDATE telegram_flows SET document = ? WHERE id = ?").bind(JSON.stringify(changed), id).run();
  assert.equal(await rejected(message(`/resume ${id}`)), "Storage");
  await database.prepare("UPDATE telegram_flows SET document = ? WHERE id = ?").bind(original.document, id).run();
});

test("roster toggles affect only selected players and fewer than two participants cannot advance", async () => {
  let reply = await success(message("/session"));
  reply = await success(callback(button(reply, "Poker")));
  reply = await success(step(reply, "2026-11-01"));
  await rejected(callback(button(reply, "Enter amounts")));
  reply = await success(callback(button(reply, "Sample Player")));
  await rejected(callback(button(reply, "Enter amounts")));
  reply = await success(callback(button(reply, "Sample Player")));
  assert.ok(!reply.text.includes("Selected:"));
  const id = reply.text.match(/Session (t\d+)/)[1];
  assert.equal(JSON.parse((await database.prepare("SELECT document FROM telegram_flows WHERE id = ?").bind(id).first()).document).selected.length, 0);
});

test("different confirmation updates racing produce exactly one ledger session", async () => {
  const { id, reply } = await review();
  const data = button(reply, "Confirm session");
  const results = await Promise.all([request(callback(data)), request(callback(data))]);
  assert.equal(results.filter(result => result.ok).length, 1);
  assert.equal((await database.prepare("SELECT COUNT(*) AS n FROM sessions WHERE id = ?").bind(id).first()).n, 1);
});

test("delivery sends a typed reply and acknowledges callbacks; failed sends replay without ledger writes", async () => {
  const { reply } = await review();
  const update = callback(button(reply, "Confirm session"));
  assert.equal((await request(update, { deliver: true, fail_send: true })).error, "Storage");
  const persisted = await snapshot();
  assert.equal((await request(update, { deliver: true, timeout_send: true })).error, "Timeout");
  assert.deepEqual(await snapshot(), persisted, "timeout retry did not duplicate writes");
  const result = await success(update, { deliver: true });
  assert.deepEqual(result.response, { method: "answerCallbackQuery", callback_query_id: update.callback_query.id });
  assert.match(result.messages[0].text, /Saved/);
  assert.deepEqual(await snapshot(), persisted, "retry after send failure did not duplicate writes");
  const invalid = await success(message("/unknown"), { deliver: true });
  assert.match(invalid.messages[0].text, /Invalid or out-of-date/);
});

test("catalog creation stops at picker capacity and concurrent requests cannot exceed it", async () => {
  let activePlayers = (await database.prepare("SELECT COUNT(*) AS n FROM players WHERE status = 'active'").first()).n;
  while (activePlayers < 23) {
    await database.prepare("INSERT INTO players (id, display_name, status) VALUES (?, 'Capacity Player', 'active')")
      .bind(`capacity_player_${activePlayers}`).run();
    activePlayers += 1;
  }
  const playerResults = await Promise.all([
    request(message("/player Capacity Left")),
    request(message("/player Capacity Right")),
  ]);
  assert.equal(playerResults.filter(result => result.ok).length, 1);
  assert.equal((await database.prepare("SELECT COUNT(*) AS n FROM players WHERE status = 'active'").first()).n, 24);
  assert.equal(await rejected(message("/player Capacity Overflow")), "Limit");

  let games = (await database.prepare("SELECT COUNT(*) AS n FROM games").first()).n;
  while (games < 24) {
    await database.prepare("INSERT INTO games (id, display_name) VALUES (?, 'Capacity Game')")
      .bind(`capacity_game_${games}`).run();
    games += 1;
  }
  assert.equal(await rejected(message("/game Capacity Overflow")), "Limit");
});
