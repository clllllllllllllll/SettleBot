import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { unstable_readConfig as readConfig } from "wrangler";

export function requireCompatibilityDate(buildDate, ...configurations) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(buildDate ?? "")
    || configurations.some(config => config?.compatibility_date !== buildDate)) {
    throw new Error("Production compatibility dates must match the local Worker build");
  }
}

export async function checkCompatibility(...configurations) {
  const build = readConfig({ config: fileURLToPath(new URL("../wrangler.toml", import.meta.url)) });
  const template = JSON.parse(await readFile(new URL("../config/deployment.json", import.meta.url), "utf8"));
  requireCompatibilityDate(build.compatibility_date, template, ...configurations);
  return build.compatibility_date;
}

if (import.meta.main) {
  try { await checkCompatibility(); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
