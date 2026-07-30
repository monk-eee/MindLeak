#!/usr/bin/env node
// Run the vitest suites that cover the root modules this push changes.
//
// `scripts/*.test.mjs` are node:test suites and run from `script-tests.mjs`.
// `editors/vscode/scripts/*.test.mjs` are vitest suites importing the very same
// modules through `../../../scripts/`, and ran only in CI. So renaming an export
// or a guidance string passed every local check and failed after publishing, on
// assertions the author had no reason to run: `droppedCommits` ->
// `classifyCommits` and `claim_task` -> `task_claim` each blocked a pull
// request that way, with all three open at once.
//
// Targeted rather than wholesale, deliberately. The full extension suite takes
// about 120s here and reports vitest worker timeouts under fleet load; a gate
// that intermittently blocks a push teaches people to reach for `--no-verify`,
// which is worse than no gate at all. One matched suite is about 18s and runs
// only when the module it covers actually changed, so an unrelated push pays
// nothing.
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "..");
const extension = join(repoRoot, "editors", "vscode");

/** The vitest suite covering a root script, when one exists. */
export const coveringSuite = (changedPath, extensionRoot = extension) => {
  const normalised = changedPath.replaceAll("\\", "/");
  if (!normalised.startsWith("scripts/") || !normalised.endsWith(".mjs")) {
    return null;
  }
  if (normalised.endsWith(".test.mjs")) return null;
  const suite = `scripts/${basename(normalised, ".mjs")}.test.mjs`;
  return existsSync(join(extensionRoot, suite)) ? suite : null;
};

/** Every suite worth running for this change set, in a stable order. */
export const suitesFor = (changedPaths, extensionRoot = extension) =>
  [
    ...new Set(
      changedPaths.map((p) => coveringSuite(p, extensionRoot)).filter(Boolean),
    ),
  ].sort();

// Importable for its own tests without launching vitest.
if (process.argv[1] && process.argv[1].endsWith("ext-test.mjs")) {
  const changed = process.argv.slice(2);
  const suites = suitesFor(changed);

  // Nothing this push touches is covered. Saying so beats silence: a hook that
  // prints nothing is indistinguishable from one that never ran, which is the
  // shape of the bug being fixed.
  if (suites.length === 0) {
    console.log(
      "ext-test: no extension suite covers the changed scripts; nothing to run",
    );
    process.exit(0);
  }

  // Absent dependencies are ordinary in a fresh worktree, not a broken build.
  // Name the command that fixes it, and refuse rather than skip: a silent skip
  // reads exactly like a green suite.
  if (!existsSync(join(extension, "node_modules"))) {
    console.error(
      "ext-test: editors/vscode/node_modules is missing, so these suites cannot run:\n" +
        suites.map((s) => `    ${s}`).join("\n") +
        "\n  Install them:  make worktree-setup   (or: npm --prefix editors/vscode ci)\n" +
        "  This refuses rather than skipping: a silent skip is indistinguishable from a green suite.",
    );
    process.exit(1);
  }

  // Git exports GIT_DIR, GIT_INDEX_FILE and friends to its hooks, and this runs
  // from pre-push. Inherited by a test that drives git in a temp directory those
  // variables outrank `cwd`, so git reads the fixture's files and writes to the
  // REAL repository. The merge-audit suite drives git in exactly that way.
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

  console.log(
    `ext-test: running ${suites.length} extension suite(s) over the changed modules`,
  );
  for (const suite of suites) console.log(`    ${suite}`);

  // `npx` is a shell script on Unix and a .cmd on Windows, so it needs a shell
  // to resolve; `shell: true` is the portable spelling, not a platform branch.
  const child = spawn("npx", ["vitest", "run", ...suites], {
    cwd: extension,
    stdio: "inherit",
    env: environment,
    shell: true,
  });

  child.on("exit", (code, signal) => {
    process.exit(signal ? 1 : (code ?? 1));
  });
}
