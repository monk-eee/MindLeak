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
import { siblingSuiteCount, siblingSuiteNotice } from "./script-suites.mjs";

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

// This runner is not the whole suite over these modules, and must say so.
// See `script-suites.mjs` for why it names the gap rather than failing on it.
const notice = siblingSuiteNotice(siblingSuiteCount(path.join(here, "..")));
if (notice) {
  console.log(notice);
}

// Git exports GIT_DIR, GIT_INDEX_FILE and friends to its hooks, and this suite
// runs from pre-push. Inherited by a test that drives git in a temp directory,
// those variables outrank `cwd`: git reads the fixture's files and writes to the
// REAL repository. That is not hypothetical — a test doing exactly this
// committed its fixtures onto the branch being pushed and left the worktree
// checked out on a branch called `theirs`, which took a while to understand
// because every symptom pointed at the fixture rather than at the environment.
//
// Scrubbing here rather than in each test is deliberate: remembering to do it
// per test is precisely the discipline that failed, and a test written next year
// gets this for free.
const environment = { ...process.env };
for (const variable of [
  "GIT_DIR",
  "GIT_WORK_TREE",
  "GIT_COMMON_DIR",
  "GIT_INDEX_FILE",
  "GIT_OBJECT_DIRECTORY",
  "GIT_ALTERNATE_OBJECT_DIRECTORIES",
]) {
  delete environment[variable];
}

const child = spawn(process.execPath, ["--test", ...tests], {
  cwd: path.join(here, ".."),
  stdio: "inherit",
  env: environment,
});

child.on("exit", (code, signal) => {
  process.exit(signal ? 1 : (code ?? 1));
});
