#!/usr/bin/env node
// Audit Lodestar goal <-> code binding coverage.
//
// Conformance decides drift by asking which goal governs a changed file. A file
// no goal binds is invisible to that question, and a binding naming a path that
// no longer exists governs nothing. Neither failure is visible from the tool
// surface: `governing_goals` reports only *active* goals, so a binding held by a
// superseded goal reads as "no goal governs this" and as "clean".
//
// Reports three things, and exits non-zero when asked to enforce:
//   1. source files under crates/*/src bound to no goal at all
//   2. bindings naming a path that does not exist on disk
//   3. bindings stranded on superseded goals, which no active clause can use
//
// Usage:
//   node scripts/binding-audit.mjs [--db <path>] [--check]
//   node scripts/binding-audit.mjs [--db <path>] --new-since <git-ref>
//
// --check exits 1 if any unbound source file or stale binding is found, so this
// can gate CI once the repository is clean.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

/** Rust source files added by this branch relative to a Git ref. */
export function addedRustSources(run, base) {
  return [
    ...new Set(
      run([
        "diff",
        "--diff-filter=A",
        "--name-only",
        `${base}...HEAD`,
        "--",
        "crates",
      ])
        .split(/\r?\n/)
        .map((file) => file.trim().replace(/\\/g, "/"))
        .filter((file) => /^crates\/[^/]+\/src\/.+\.rs$/.test(file)),
    ),
  ].sort();
}

/** Added source files for which the ledger has no artifact binding. */
export function unboundSources(files, boundNodeIds) {
  const bound = new Set(boundNodeIds);
  return files.filter((file) => !bound.has(`artifact:${file}`)).sort();
}

/** The current repository's configured state database, when it exists. */
export function configuredRepositoryDb(run, exists, repositories) {
  let repositoryId;
  try {
    repositoryId = run([
      "config",
      "--local",
      "--get",
      "mindleak.repositoryId",
    ]).trim();
  } catch {
    return null;
  }
  if (!/^[0-9a-f]{32}$/.test(repositoryId)) return null;
  const candidate = path.join(repositories, repositoryId, "spec.db");
  return exists(candidate) ? candidate : null;
}

/** Where MindLeak keeps per-repository state on each platform. */
function stateRoot() {
  if (process.env.MINDLEAK_STATE_DIR) return process.env.MINDLEAK_STATE_DIR;
  if (process.platform === "win32") {
    const base =
      process.env.LOCALAPPDATA ?? path.join(os.homedir(), "AppData", "Local");
    return path.join(base, "MindLeak");
  }
  if (process.platform === "darwin") {
    return path.join(
      os.homedir(),
      "Library",
      "Application Support",
      "MindLeak",
    );
  }
  return path.join(
    process.env.XDG_DATA_HOME ?? path.join(os.homedir(), ".local", "share"),
    "MindLeak",
  );
}

function resolveDb(argv, repoRoot) {
  const dbFlag = argv.indexOf("--db");
  if (dbFlag !== -1 && argv[dbFlag + 1]) return argv[dbFlag + 1];
  const repositories = path.join(stateRoot(), "repositories");
  if (!fs.existsSync(repositories)) {
    throw new Error(
      `binding-audit: no repository state at ${repositories}. Pass --db <path to spec.db>; storage_status reports it.`,
    );
  }
  const configured = configuredRepositoryDb(
    (args) =>
      execFileSync("git", args, { cwd: repoRoot, encoding: "utf8" }).trim(),
    fs.existsSync,
    repositories,
  );
  if (configured) return configured;
  const found = fs
    .readdirSync(repositories)
    .map((id) => path.join(repositories, id, "spec.db"))
    .filter((p) => fs.existsSync(p));
  if (found.length === 1) return found[0];
  throw new Error(
    found.length === 0
      ? `binding-audit: no spec.db under ${repositories}. Pass --db <path>.`
      : `binding-audit: ${found.length} repositories have state; pass --db <path>:\n  ${found.join("\n  ")}`,
  );
}

/** Every .rs file under a crate's src directory. */
function sources(dir, out = []) {
  if (!fs.existsSync(dir)) return out;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) sources(full, out);
    else if (entry.name.endsWith(".rs")) out.push(full);
  }
  return out;
}

async function main() {
  let DatabaseSync;
  try {
    ({ DatabaseSync } = await import("node:sqlite"));
  } catch {
    throw new Error(
      "binding-audit: this needs Node's built-in sqlite (Node 22.5+). Upgrade Node, or pass --db to a build that has it.",
    );
  }

  const argv = process.argv.slice(2);
  const check = argv.includes("--check");
  const newSinceFlag = argv.indexOf("--new-since");
  if (newSinceFlag !== -1 && !argv[newSinceFlag + 1]) {
    throw new Error("binding-audit: --new-since requires a Git ref");
  }
  const repoRoot = path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    "..",
  );
  const dbPath = resolveDb(argv, repoRoot);
  const db = new DatabaseSync(dbPath, { readOnly: true });
  try {
    const bindings = db
      .prepare("select goal_id, node_id, mode from goal_artifacts")
      .all();
    const bound = new Set(bindings.map((binding) => binding.node_id));

    if (newSinceFlag !== -1) {
      const base = argv[newSinceFlag + 1];
      const run = (args) =>
        execFileSync("git", args, { cwd: repoRoot, encoding: "utf8" }).trim();
      const added = addedRustSources(run, base);
      const missing = unboundSources(added, bound);
      const missingSet = new Set(missing);
      console.log(
        `=== binding coverage: Rust source files added since ${base} ===`,
      );
      if (added.length === 0) console.log("  none");
      for (const file of added) {
        console.log(
          `  ${missingSet.has(file) ? "UNBOUND" : "BOUND  "}  ${file}`,
        );
      }
      console.log(
        `summary: ${missing.length} of ${added.length} newly added Rust source files are unbound`,
      );
      return;
    }

    const goals = new Map(
      db
        .prepare("select id, status, superseded_by from goals")
        .all()
        .map((goal) => [goal.id, goal]),
    );
    const crates = fs.existsSync(path.join(repoRoot, "crates"))
      ? fs.readdirSync(path.join(repoRoot, "crates"))
      : [];

    let unbound = 0;
    console.log("=== coverage: source files bound to a goal ===");
    for (const crate of crates) {
      const src = path.join(repoRoot, "crates", crate, "src");
      const files = sources(src).map((file) =>
        path.relative(repoRoot, file).split(path.sep).join("/"),
      );
      if (files.length === 0) continue;
      const missing = unboundSources(files, bound);
      unbound += missing.length;
      console.log(
        `  ${String(files.length - missing.length).padStart(3)}/${String(files.length).padEnd(3)}  crates/${crate}/src`,
      );
      for (const file of missing) console.log(`        UNBOUND  ${file}`);
    }

    console.log("\n=== bindings naming a path that no longer exists ===");
    let stale = 0;
    for (const binding of bindings) {
      if (!binding.node_id.startsWith("artifact:")) continue;
      const rel = binding.node_id.slice("artifact:".length);
      if (!fs.existsSync(path.join(repoRoot, rel))) {
        stale += 1;
        console.log(`  MISSING  ${rel}   (${binding.goal_id})`);
      }
    }
    if (stale === 0) console.log("  none");

    console.log("\n=== bindings stranded on superseded goals ===");
    const stranded = bindings.filter(
      (binding) =>
        (goals.get(binding.goal_id)?.status ?? "unknown") !== "active",
    );
    if (stranded.length === 0) {
      console.log("  none");
    } else {
      const byGoal = new Map();
      for (const binding of stranded) {
        byGoal.set(binding.goal_id, (byGoal.get(binding.goal_id) ?? 0) + 1);
      }
      for (const [goal, count] of [...byGoal].sort((a, b) => b[1] - a[1])) {
        const successor = goals.get(goal)?.superseded_by;
        console.log(
          `  ${String(count).padStart(3)}  ${goal}${successor ? `  -> ${successor}` : "  (no superseded_by recorded)"}`,
        );
      }
      console.log(
        `  ${stranded.length} of ${bindings.length} bindings cannot be reached by any active clause.`,
      );
    }

    console.log(
      `\nsummary: ${unbound} unbound source files, ${stale} stale bindings, ${stranded.length} stranded bindings (db ${dbPath})`,
    );
    if (check && (unbound > 0 || stale > 0)) process.exitCode = 1;
  } finally {
    db.close();
  }
}

const invoked =
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invoked) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 2;
  });
}
