import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { createTestHarness } from "wrangler";
import { deploymentConfig } from "../scripts/prepare-deployment.mjs";
import { verifyRelease } from "../scripts/release-artifact.mjs";
import { smoke } from "../scripts/smoke.mjs";

const template = JSON.parse(await readFile("config/deployment.json", "utf8"));
const manifest = JSON.parse(await readFile(".release/manifest.json", "utf8"));
const sha = process.env.GITHUB_SHA ?? manifest.sha;
const secrets = {
  TELEGRAM_WEBHOOK_SECRET: "fake_secret", TELEGRAM_CHAT_ID: "-100",
  TELEGRAM_ADMIN_IDS: "100", TELEGRAM_BOT_USERNAME: "SettleBot",
  TELEGRAM_BOT_TOKEN: "500:fake_test_token", EXPORT_SECRET: "fake_export_secret",
};
const identities = {
  CLOUDFLARE_ACCOUNT_ID: "1".repeat(32),
  STAGING_D1_ID: "11111111-1111-4111-8111-111111111111",
  PRODUCTION_D1_ID: "22222222-2222-4222-8222-222222222222",
};

for (const target of ["staging", "production"]) {
  test(`packaged ${target} Worker rejects missing schema and passes migrated local smoke`, async () => {
    await verifyRelease(".release", sha);
    const directory = await mkdtemp(join(tmpdir(), "settlebot-release-"));
    const config = deploymentConfig(template, target, sha, identities);
    // Only the pinned local runtime needs this older compatibility date.
    config.compatibility_date = "2026-10-13";
    const secretValues = Object.entries(secrets).map(([key, value]) => `${key}=${JSON.stringify(value)}`);
    await writeFile(join(directory, ".dev.vars"), secretValues.join("\n"));
    const configPath = join(directory, "wrangler.json");
    await writeFile(configPath, JSON.stringify(config));
    const server = createTestHarness({ workers: [{ configPath }] });
    try {
      await server.listen();
      const input = { target, sha, origin: `https://settlebot-${target}.example.workers.dev`,
        exportToken: secrets.EXPORT_SECRET };
      const fetcher = (url, init) => server.fetch(url, init);
      await assert.rejects(smoke(input, fetcher), /Authenticated D1 export failed/);
      await server.getWorker().applyD1Migrations("LEDGER_DB");
      await smoke(input, fetcher);
      const { LEDGER_DB: database } = await server.getWorker().getEnv();
      assert.equal(await database.prepare("SELECT COUNT(*) FROM sessions").first("COUNT(*)"), 0);
      assert.equal(await database.prepare("SELECT COUNT(*) FROM session_audits").first("COUNT(*)"), 0);
    } finally {
      await server.close();
      await rm(directory, { recursive: true, force: true });
    }
  });
}
