// Every language must define every French key, and every t("key") used in the UI must exist.
const src = (await import("node:fs")).readFileSync("src/i18n.js", "utf8");
const dicts = {};
for (const m of src.matchAll(/^const (\w+) = \{([\s\S]*?)^\};/gm)) dicts[m[1]] = new Set([...m[2].matchAll(/(\w+):\s*"/g)].map((x) => x[1]));
const fr = dicts.fr; let bad = 0;
for (const [k, d] of Object.entries(dicts)) for (const key of fr) if (!d.has(key)) { console.log(k, "missing", key); bad++; }
// keys used in code
const fs = await import("node:fs");
for (const f of ["src/main.js", "src/hosts.js", "src/panes.js", "src/panel.js", "src/ui.js", "src/sync.js", "src/keys.js"]) {
  for (const m of fs.readFileSync(f, "utf8").matchAll(/\b(?:t|tr)\("(\w+)"[,)]/g)) if (!fr.has(m[1])) { console.log(f, "unknown key", m[1]); bad++; }
}
console.log(Object.fromEntries(Object.entries(dicts).map(([k, d]) => [k, d.size])), bad ? "FAIL" : "OK");
process.exit(bad ? 1 : 0);
