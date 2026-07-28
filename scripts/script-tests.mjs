#!/usr/bin/env node
// Run the repository's own script tests.
//
// These cover the machinery the fleet relies on to stay honest: the
// conformance gate, the merge-driver guard, the claim gate, the publication
// record, the delivery queue and the board health report. They were written
// with `node --test scripts/` in a header comment and wired into nothing —
// no CI job, no Makefile target, no hook — so 45 assertions about the guards
// ran only when somebody remembered to type the command by hand.
//
// The invocation is a runner rather than a one-liner because the one-liner is
// not portable across the versions in play: passing a directory works on
// Node 20 and fails on Node 24, while a glob pattern works on Node 24 and not
// on the Node 20 that CI pins. Enumerating the files and passing them
// explicitly works on both, on every OS.
import { spawn } from "node:child_process";
import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));

const tests = readdirSync(here)
  .filter((name) => name.endsWith(".test.mjs"))
  .sort()
  .map((name) => path.join("scripts", name));

// A runner that discovers nothing must not report success. Silence here would
// look exactly like a green suite, which is the failure it exists to prevent.
if (tests.length === 0) {
  console.error(
    "script-tests: found no scripts/*.test.mjs — the runner is looking in the wrong place",
  );
  process.exit(1);
}

console.log(`script-tests: running ${tests.length} test files`);

const child = spawn(process.execPath, ["--test", ...tests], {
  cwd: path.join(here, ".."),
  stdio: "inherit",
});

child.on("exit", (code, signal) => {
  process.exit(signal ? 1 : (code ?? 1));
});
