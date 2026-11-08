import { validateSha } from "./release-artifact.mjs";

const MAX_RESPONSE_BYTES = 256 * 1024;
const REQUEST_TIMEOUT_MS = 10_000;

export function smokeOrigin(value, target) {
  const url = new URL(value);
  if (!["staging", "production"].includes(target) || url.protocol !== "https:"
      || url.username || url.password || url.port || url.pathname !== "/" || url.search || url.hash
      || !new RegExp(`^settlebot-${target}\\.[a-z0-9-]+\\.workers\\.dev$`).test(url.hostname)) {
    throw new Error("Expected the selected environment's workers.dev origin");
  }
  return url;
}

async function readJson(response) {
  let bytes = 0;
  const chunks = [];
  for await (const chunk of response.body) {
    bytes += chunk.length;
    if (bytes > MAX_RESPONSE_BYTES) throw new Error("Smoke response exceeds limit");
    chunks.push(chunk);
  }
  return JSON.parse(Buffer.concat(chunks).toString("utf8"));
}

async function request(fetcher, origin, path, options = {}) {
  return fetcher(new URL(path, origin), {
    ...options, redirect: "error", signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    cache: "no-store",
  });
}

export async function smoke({ origin, target, sha, exportToken }, fetcher = fetch) {
  validateSha(sha);
  const base = smokeOrigin(origin, target);
  if (Buffer.byteLength(exportToken ?? "") < 16) throw new Error("Export smoke credential is required");
  const health = await request(fetcher, base, "/health");
  if (health.status !== 200 || health.headers.get("x-settlebot-commit") !== sha
      || health.headers.get("cache-control") !== "no-store"
      || (await readJson(health)).status !== "ok") throw new Error("Health or release identity failed");
  await rejectedRequest(fetcher, base, "/admin/export");
  await rejectedRequest(fetcher, base, "/telegram/webhook", { method: "POST", body: "{}" });
  const exported = await request(fetcher, base, "/admin/export", {
    headers: { authorization: `Bearer ${exportToken}` },
  });
  if (exported.status !== 200 || exported.headers.get("cache-control") !== "no-store") {
    throw new Error("Authenticated D1 export failed");
  }
  const page = await readJson(exported);
  if (page.format !== "settlebot-ledger-v1"
      || ![page.players, page.games, page.sessions].every(Array.isArray)) {
    throw new Error("Unexpected ledger export format");
  }
}

async function rejectedRequest(fetcher, base, path, options) {
  const response = await request(fetcher, base, path, options);
  if (response.status !== 403) throw new Error("Authorization smoke failed");
  await response.body?.cancel();
}

if (import.meta.main) {
  try {
    await smoke({ target: process.argv[2], sha: process.argv[3],
      origin: process.env.WORKER_ORIGIN, exportToken: process.env.EXPORT_SECRET });
    console.log("Release identity, health, authorization, and D1 export smoke passed");
  } catch {
    // Never log response bodies, URLs, headers, or fetch errors containing private data.
    console.error("Deployment smoke failed; inspect environment configuration and service health");
    process.exitCode = 1;
  }
}
