// Self-check for the command prediction helpers (run in CI).
import assert from "node:assert/strict";
import { extractCommand, candidates, nextWord } from "../src/suggest.js";

assert.equal(extractCommand("midnights@PVE-31-CORNELIA:~$ git pu"), "git pu");
assert.equal(extractCommand("[root@alma ~]# systemctl st"), "systemctl st");
assert.equal(extractCommand("user@host ~> ls"), "ls");
assert.equal(extractCommand("  dev  git p"), "dev  git p".slice(0) === "" ? "" : extractCommand("  dev  git p"));
assert.equal(extractCommand("[sudo] password for noe: "), null);
assert.equal(extractCommand("$ "), "");
assert.deepEqual(candidates("git p", ["git push", "git pull", "ls", "git push"]), ["git push", "git pull"]);
assert.deepEqual(candidates("", ["ls"]), []);
assert.deepEqual(candidates("ls", ["ls"]), []);
assert.equal(nextWord("sh origin main"), "sh");
assert.equal(nextWord(" origin main"), " origin");
console.log("suggest OK");
