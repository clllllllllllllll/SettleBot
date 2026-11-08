import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { validateSha, verifyRelease } from "./release-artifact.mjs";

const UUID = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/i;
const ACCOUNT = /^[a-f0-9]{32}$/i;
const ZERO_UUID = "00000000-0000-0000-0000-000000000000";

export function deploymentConfig(template, target, sha, values) {
  validateSha(sha);
  if (!["staging", "production"].includes(target)) throw new Error("Unknown deployment environment");
  const { CLOUDFLARE_ACCOUNT_ID: account, STAGING_D1_ID: staging, PRODUCTION_D1_ID: production } = values;
  if (!ACCOUNT.test(account ?? "") || /^0+$/.test(account)) throw new Error("Invalid account ID");
  if (![staging, production].every((id) => UUID.test(id ?? "") && id !== ZERO_UUID)) {
    throw new Error("Both environment database IDs are required");
  }
  if (staging.toLowerCase() === production.toLowerCase()) throw new Error("Databases must be isolated");
  return {
    ...template, name: `settlebot-${target}`, account_id: account,
    main: resolve(".release/index.js"), vars: { RELEASE_SHA: sha },
    d1_databases: [{ binding: "LEDGER_DB", database_name: `settlebot-${target}`,
      database_id: target === "staging" ? staging : production, migrations_dir: resolve("migrations") }],
  };
}

if (import.meta.main) {
  try {
    const [target, sha] = process.argv.slice(2);
    await verifyRelease(".release", sha);
    const template = JSON.parse(await readFile("config/deployment.json", "utf8"));
    const config = deploymentConfig(template, target, sha, process.env);
    await mkdir(".wrangler", { recursive: true });
    await writeFile(`.wrangler/${target}.json`, JSON.stringify(config, null, 2), { mode: 0o600 });
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
