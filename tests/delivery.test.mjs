import assert from "node:assert/strict";
import { mkdtemp, readFile, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { deploymentConfig } from "../scripts/prepare-deployment.mjs";
import { packageRelease, verifyRelease } from "../scripts/release-artifact.mjs";
import { smoke, smokeOrigin } from "../scripts/smoke.mjs";
import { requireReviewers, verifyProductionGate } from "../scripts/check-production-gate.mjs";

const SHA = "1234567890abcdef1234567890abcdef12345678";
const values = {
  CLOUDFLARE_ACCOUNT_ID: "1".repeat(32),
  STAGING_D1_ID: "11111111-1111-4111-8111-111111111111",
  PRODUCTION_D1_ID: "22222222-2222-4222-8222-222222222222",
};
const template = JSON.parse(await readFile(new URL("../config/deployment.json", import.meta.url)));

test("package commands expose dry-run builds without a generic Worker deploy command", async () => {
  const { scripts } = JSON.parse(await readFile(new URL("../package.json", import.meta.url)));
  assert.equal(scripts.deploy, undefined);
  assert.match(scripts.build, /^wrangler deploy --dry-run --outdir dist$/);
  const action = await readFile(new URL("../.github/actions/deploy/action.yml", import.meta.url), "utf8");
  assert.match(action, /wrangler deploy --config/);
});

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
    { protection_rules: null }, { protection_rules: [{ type: "required_reviewers", reviewers: [null, {}] }] },
    { protection_rules: [{ type: "required_reviewers", reviewers: [] }] }]) {
    assert.throws(() => requireReviewers(config));
  }
  requireReviewers(protection());
});

function protection(prevent_self_review = false) {
  return { can_admins_bypass: false,
    protection_rules: [{ type: "required_reviewers", prevent_self_review,
      reviewers: [{ type: "User", reviewer: { id: 1 } }] }],
    deployment_branch_policy: { protected_branches: false, custom_branch_policies: true } };
}
const mainOnly = () => ({ total_count: 1, branch_policies: [{ id: 1, name: "main", type: "branch" }] });

function gateResponses() {
  return { production: protection(), "production/deployment-branch-policies": mainOnly(),
    staging: { deployment_branch_policy: protection().deployment_branch_policy },
    "staging/deployment-branch-policies": mainOnly() };
}

function gateReader(responses, requests = []) {
  return async (url, init) => {
    const path = new URL(url).pathname.split("/environments/")[1];
    requests.push([url, init]);
    return json(responses[path]);
  };
}

test("approval preflight reads actual environment and policy endpoints with bounded GETs", async () => {
  for (const selfReview of [false, true]) {
    const data = gateResponses(); data.production = protection(selfReview);
    const requests = [];
    await verifyProductionGate("sample/repository", "fake_read_token", gateReader(data, requests));
    assert.equal(requests.length, 4);
    assert.ok(requests.every(([, init]) => init.method === "GET" && init.redirect === "error" && init.signal));
    assert.ok(requests.filter(([url]) => url.includes("deployment-branch-policies")).every(([url]) =>
      new URL(url).searchParams.get("per_page") === "100"));
  }
});

test("approval preflight fails closed on missing, bypassable, broad, protected-only and tag policies", async () => {
  const invalid = [
    ["production", {}], ["production", { ...protection(), can_admins_bypass: true }],
    ["production", { ...protection(), can_admins_bypass: undefined }],
    ["production", { ...protection(), protection_rules: [] }],
    ["production", { ...protection(), deployment_branch_policy: { protected_branches: true, custom_branch_policies: false } }],
    ["production", { ...protection(), deployment_branch_policy: null }],
    ["staging", {}],
  ];
  for (const target of ["production", "staging"]) {
    for (const policy of [{}, { total_count: 0, branch_policies: [] },
      { total_count: 2, branch_policies: [mainOnly().branch_policies[0], { name: "release/*", type: "branch" }] },
      { total_count: 1, branch_policies: [{ name: "*", type: "branch" }] },
      { total_count: 1, branch_policies: [{ name: "main", type: "tag" }] },
      { total_count: 1, branch_policies: [{ name: "main" }] },
      { total_count: 101, branch_policies: mainOnly().branch_policies }]) {
      invalid.push([`${target}/deployment-branch-policies`, policy]);
    }
  }
  for (const [path, value] of invalid) {
    const data = gateResponses(); data[path] = value;
    await assert.rejects(verifyProductionGate("sample/repository", "fake_read_token", gateReader(data)));
  }
});

test("approval preflight refuses inaccessible, malformed, timed-out or unavailable API reads", async () => {
  for (const failure of [async () => json({}, 403), async () => json({}, 404),
    async () => json({}, 500), async () => new Response("not json"),
    async () => { throw new TypeError("network unavailable"); },
    async () => { throw new DOMException("deadline", "TimeoutError"); }]) {
    await assert.rejects(verifyProductionGate("sample/repository", "fake_read_token", failure));
  }
  const data = gateResponses();
  await assert.rejects(verifyProductionGate("sample/repository", "fake_read_token", async (url, init) =>
    url.includes("deployment-branch-policies") ? json({}, 403) : gateReader(data)(url, init)));
  await assert.rejects(verifyProductionGate("sample/repository", "", gateReader(data)));
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
