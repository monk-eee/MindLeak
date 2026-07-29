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
//
// --check exits 1 if any unbound source file or stale binding is found, so this
// can gate CI once the repository is clean.

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";

let DatabaseSync;
try {
  ({ DatabaseSync } = await import("node:sqlite"));
} catch {
  console.error(
    "binding-audit: this needs Node's built-in sqlite (Node 22.5+). Upgrade Node, or pass --db to a build that has it.",
  );
  process.exit(2);
}

const argv = process.argv.slice(2);
const check = argv.includes("--check");
const dbFlag = argv.indexOf("--db");
const repoRoot = path.resolve(import.meta.dirname, "..");

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

function resolveDb() {
  if (dbFlag !== -1 && argv[dbFlag + 1]) return argv[dbFlag + 1];
  const repositories = path.join(stateRoot(), "repositories");
  if (!fs.existsSync(repositories)) {
    console.error(
      `binding-audit: no repository state at ${repositories}. Pass --db <path to spec.db>; storage_status reports it.`,
    );
    process.exit(2);
  }
  const found = fs
    .readdirSync(repositories)
    .map((id) => path.join(repositories, id, "spec.db"))
    .filter((p) => fs.existsSync(p));
  if (found.length === 1) return found[0];
  console.error(
    found.length === 0
      ? `binding-audit: no spec.db under ${repositories}. Pass --db <path>.`
      : `binding-audit: ${found.length} repositories have state; pass --db <path>:\n  ${found.join("\n  ")}`,
  );
  process.exit(2);
}

const dbPath = resolveDb();
const db = new DatabaseSync(dbPath, { readOnly: true });

const bindings = db
  .prepare("select goal_id, node_id, mode from goal_code")
  .all();
const goals = new Map(
  db
    .prepare("select id, status, superseded_by from goals")
    .all()
    .map((g) => [g.id, g]),
);

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

const bound = new Set(bindings.map((b) => b.node_id));
const crates = fs.existsSync(path.join(repoRoot, "crates"))
  ? fs.readdirSync(path.join(repoRoot, "crates"))
  : [];

let unbound = 0;
console.log("=== coverage: source files bound to a goal ===");
for (const crate of crates) {
  const src = path.join(repoRoot, "crates", crate, "src");
  const files = sources(src).map((f) =>
    path.relative(repoRoot, f).split(path.sep).join("/"),
  );
  if (files.length === 0) continue;
  const missing = files.filter((f) => !bound.has(`artifact:${f}`));
  unbound += missing.length;
  console.log(
    `  ${String(files.length - missing.length).padStart(3)}/${String(files.length).padEnd(3)}  crates/${crate}/src`,
  );
  for (const f of missing) console.log(`        UNBOUND  ${f}`);
}

console.log("\n=== bindings naming a path that no longer exists ===");
let stale = 0;
for (const b of bindings) {
  if (!b.node_id.startsWith("artifact:")) continue;
  const rel = b.node_id.slice("artifact:".length);
  if (!fs.existsSync(path.join(repoRoot, rel))) {
    stale += 1;
    console.log(`  MISSING  ${rel}   (${b.goal_id})`);
  }
}
if (stale === 0) console.log("  none");

console.log("\n=== bindings stranded on superseded goals ===");
const stranded = bindings.filter(
  (b) => (goals.get(b.goal_id)?.status ?? "unknown") !== "active",
);
if (stranded.length === 0) {
  console.log("  none");
} else {
  const byGoal = new Map();
  for (const b of stranded)
    byGoal.set(b.goal_id, (byGoal.get(b.goal_id) ?? 0) + 1);
  for (const [goal, n] of [...byGoal].sort((a, b) => b[1] - a[1])) {
    const successor = goals.get(goal)?.superseded_by;
    console.log(
      `  ${String(n).padStart(3)}  ${goal}${successor ? `  -> ${successor}` : "  (no superseded_by recorded)"}`,
    );
  }
  console.log(
    `  ${stranded.length} of ${bindings.length} bindings cannot be reached by any active clause.`,
  );
}

console.log(
  `\nsummary: ${unbound} unbound source files, ${stale} stale bindings, ${stranded.length} stranded bindings (db ${dbPath})`,
);

if (check && (unbound > 0 || stale > 0)) process.exit(1);
