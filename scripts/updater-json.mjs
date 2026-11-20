// Builds latest.json (Tauri updater manifest) from the assets of a GitHub release.
// Usage: node scripts/updater-json.mjs <version> <notes> <assets-dir> <download-base-url>
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [version, notes, dir, base] = process.argv.slice(2);
const files = readdirSync(dir);
const PLATFORMS = [
  ["darwin-aarch64", /aarch64\.app\.tar\.gz$/],
  ["darwin-x86_64", /x64\.app\.tar\.gz$/],
  ["linux-x86_64", /amd64\.AppImage$/],
  ["linux-aarch64", /aarch64\.AppImage$/],
  ["windows-x86_64", /x64-setup\.exe$/],
  ["windows-aarch64", /arm64-setup\.exe$/],
];
const platforms = {};
for (const [key, re] of PLATFORMS) {
  // Only the .sig files are downloaded; the bundle name is the sig name without ".sig".
  const file = files.filter((f) => f.endsWith(".sig")).map((f) => f.slice(0, -4)).find((f) => re.test(f));
  if (!file) { console.error(`missing updater bundle for ${key}`); continue; }
  platforms[key] = { signature: readFileSync(join(dir, file + ".sig"), "utf8").trim(), url: `${base}/${encodeURIComponent(file)}` };
}
if (!Object.keys(platforms).length) { console.error("no updater bundles found"); process.exit(1); }
writeFileSync(join(dir, "latest.json"), JSON.stringify({ version, notes, pub_date: new Date().toISOString(), platforms }, null, 2));
console.log("latest.json:", Object.keys(platforms).join(", "));
