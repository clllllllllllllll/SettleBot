import assert from "node:assert/strict";
import { mkdtemp, readFile, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { deploymentConfig } from "../scripts/prepare-deployment.mjs";
import { packageRelease, verifyRelease } from "../scripts/release-artifact.mjs";
import { smoke, smokeOrigin } from "../scripts/smoke.mjs";
import { requireReviewers } from "../scripts/check-production-gate.mjs";

const SHA = "1234567890abcdef1234567890abcdef12345678";
const values = {
  CLOUDFLARE_ACCOUNT_ID: "1".repeat(32),
  STAGING_D1_ID: "11111111-1111-4111-8111-111111111111",
  PRODUCTION_D1_ID: "22222222-2222-4222-8222-222222222222",
};
const template = JSON.parse(await readFile(new URL("../config/deployment.json", import.meta.url)));

test("configuration isolates environments and rejects missing, shared, and placeholder IDs", () => {
  const staging = deploymentConfig(template, "staging", SHA, values);
  const production = deploymentConfig(template, "production", SHA, values);
  assert.notEqual(staging.name, production.name);
  assert.equal(staging.d1_databases[0].database_id, values.STAGING_D1_ID);
  assert.equal(production.d1_databases[0].database_id, values.PRODUCTION_D1_ID);
  assert.equal(staging.vars.RELEASE_SHA, SHA);
  assert.equal(staging.build, undefined);
  assert.equal(staging.no_bundle, true);
  for (const invalid of [{}, { ...values, PRODUCTION_D1_ID: values.STAGING_D1_ID },
    { ...values, STAGING_D1_ID: "00000000-0000-0000-0000-000000000000" }]) {
    assert.throws(() => deploymentConfig(template, "staging", SHA, invalid));
  }
  assert.throws(() => deploymentConfig(template, "local", SHA, values));
  assert.throws(() => deploymentConfig(template, "staging", "main", values));
});

test("artifact excludes stale Wasm files and rejects wrong commits, corruption, and extra files", async () => {
  const directory = await mkdtemp(join(tmpdir(), "settlebot-delivery-"));
  try {
    await writeFile(join(directory, "index.js"), 'import wasm from "./abc123-index_bg.wasm";');
    await writeFile(join(directory, "abc123-index_bg.wasm"), "sample module");
    await writeFile(join(directory, "old-index_bg.wasm"), "stale module");
    const release = join(directory, "release");
    await packageRelease(directory, release, SHA);
    await verifyRelease(release, SHA);
    await assert.rejects(verifyRelease(release, "f".repeat(40)), /commit mismatch/);
    await writeFile(join(release, "extra.js"), "unexpected");
    await assert.rejects(verifyRelease(release, SHA), /Unexpected release files/);
    await rm(join(release, "extra.js"));
    await writeFile(join(release, "abc123-index_bg.wasm"), "corrupted");
    await assert.rejects(verifyRelease(release, SHA), /checksum mismatch/);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("missing production reviewers fail closed", () => {
  for (const config of [{}, { protection_rules: [] },
    { protection_rules: [{ type: "required_reviewers", reviewers: [] }] }]) {
    assert.throws(() => requireReviewers(config));
  }
  requireReviewers({ protection_rules: [{ type: "required_reviewers", reviewers: [{ id: 1 }] }] });
});

const options = { origin: "https://settlebot-staging.example.workers.dev", target: "staging",
  sha: SHA, exportToken: "fake_smoke_export_secret" };
const json = (body, status = 200, headers = {}) => new Response(JSON.stringify(body), {
  status, headers: { "content-type": "application/json", ...headers },
});

function responses() {
  return [
    json({ status: "ok" }, 200, { "x-settlebot-commit": SHA, "cache-control": "no-store" }),
    json({ error: "Forbidden" }, 403), json({ error: "Forbidden" }, 403),
    json({ format: "settlebot-ledger-v1", players: [], games: [], sessions: [] }, 200,
      { "cache-control": "no-store" }),
  ];
}

test("smoke checks SHA, rejects unauthenticated access, and only reads ledger data", async () => {
  const queue = responses();
  const requests = [];
  await smoke(options, async (url, init) => { requests.push([url, init]); return queue.shift(); });
  assert.equal(requests.length, 4);
  assert.deepEqual(requests.map(([url]) => url.pathname),
    ["/health", "/admin/export", "/telegram/webhook", "/admin/export"]);
  assert.equal(requests[2][1].headers, undefined);
  assert.equal(requests[2][1].body, "{}");
  assert.equal(requests[3][1].headers.authorization, `Bearer ${options.exportToken}`);
  assert.ok(requests.every(([, init]) => init.redirect === "error" && init.signal));
});

test("smoke fails on another release, authorization bypass, schema failure, or cached export", async () => {
  for (const [index, replacement] of [
    [0, () => json({ status: "ok" }, 200, { "x-settlebot-commit": "f".repeat(40) })],
    [1, () => json({ format: "settlebot-ledger-v1" })],
    [2, () => json({ error: "Unavailable" }, 503)],
    [3, () => json({ error: "Unavailable" }, 503)],
    [3, () => json({ format: "settlebot-ledger-v1" })],
  ]) {
    const queue = responses();
    queue[index] = replacement();
    await assert.rejects(smoke(options, async () => queue.shift()));
  }
});

test("smoke refuses insecure, credential-bearing, or mismatched environment URLs", () => {
  for (const origin of ["http://settlebot-staging.example.workers.dev",
    "https://settlebot-production.example.workers.dev", "https://attacker.example",
    `${options.origin}/path`, `${options.origin}?token=private`,
    "https://user:secret@settlebot-staging.example.workers.dev"]) {
    assert.throws(() => smokeOrigin(origin, "staging"));
  }
});
