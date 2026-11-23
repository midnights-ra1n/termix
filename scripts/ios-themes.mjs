// Built-in themes (src/themes.js) for the native iOS app, in order: node scripts/ios-themes.mjs
import { writeFileSync } from "node:fs";
import { THEMES } from "../src/themes.js";

const list = Object.entries(THEMES).map(([id, t]) => ({ id, ...t }));
writeFileSync(new URL("../ios/Termix/themes.json", import.meta.url), JSON.stringify(list, null, 1) + "\n");
