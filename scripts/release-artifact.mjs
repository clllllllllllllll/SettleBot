import { createHash } from "node:crypto";
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

export function validateSha(sha) {
  if (!/^[a-f0-9]{40}$/.test(sha ?? "")) throw new Error("A full commit SHA is required");
  return sha;
}

const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");

function moduleNames(source) {
  const matches = [...source.matchAll(/from\s*["']\.\/([a-f0-9]+-index_bg\.wasm)["']/g)];
  if (matches.length !== 1) throw new Error("Expected one bundled Wasm module");
  return ["index.js", matches[0][1]];
}

export async function packageRelease(source, destination, sha) {
  validateSha(sha);
  const entry = await readFile(join(source, "index.js"), "utf8");
  const files = {};
  await mkdir(destination, { recursive: true });
  if ((await readdir(destination)).length) throw new Error("Release directory must be empty");
  for (const name of moduleNames(entry)) {
    const bytes = await readFile(join(source, name));
    files[name] = digest(bytes);
    await writeFile(join(destination, name), bytes, { flag: "wx" });
  }
  await writeFile(join(destination, "manifest.json"), JSON.stringify({ sha, files }, null, 2));
}

export async function verifyRelease(directory, sha) {
  validateSha(sha);
  const manifest = JSON.parse(await readFile(join(directory, "manifest.json"), "utf8"));
  if (manifest.sha !== sha) throw new Error("Release commit mismatch");
  const source = await readFile(join(directory, "index.js"), "utf8");
  const expected = moduleNames(source).sort();
  if (JSON.stringify(Object.keys(manifest.files).sort()) !== JSON.stringify(expected)) {
    throw new Error("Release module list mismatch");
  }
  const actual = (await readdir(directory)).filter((name) => name !== "manifest.json").sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error("Unexpected release files");
  for (const name of expected) {
    if (digest(await readFile(join(directory, name))) !== manifest.files[name]) {
      throw new Error("Release checksum mismatch");
    }
  }
}

if (import.meta.main) {
  try {
    const [operation, sha] = process.argv.slice(2);
    if (operation === "package") await packageRelease("dist", ".release", sha);
    else if (operation === "verify") await verifyRelease(".release", sha);
    else throw new Error("Expected package or verify");
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
