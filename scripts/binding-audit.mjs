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
//   node scripts/binding-audit.mjs [--db <path>] [--check] [--repair]
//   node scripts/binding-audit.mjs [--db <path>] --new-since <git-ref>
//
// --check exits 1 if any unbound source file or stale binding is found, so this
// can gate CI once the repository is clean.
//
// --repair applies the one repair that needs no judgement: a binding whose file
// became a same-named module directory is moved onto the descendants, goal and
// mode unchanged, and the dead path removed. Four occurrences of that shape are
// recorded in gaps.d, each fixed identically by hand. Everything else is
// reported and left alone — a genuine deletion has no successor to guess at.
// The plan is always printed first, so --repair applies exactly what a
// plain run just showed you.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import { callTools, resolveServer } from "./claim-gate.mjs";

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

/**
 * Where a bound path still exists, or `null` when nothing holds it.
 *
 * A binding lives in the per-repository `spec.db` that every linked worktree
 * shares, but the path it names is resolved against whichever working tree the
 * audit happens to run in. Those are different scopes, and treating them as one
 * makes the verdict depend on where you stood. Measured 2026-08-29 against one
 * unchanged database: from a checkout at `origin/main`, 4 stale bindings; from
 * the worktree holding the unmerged branch that adds those files, 0. All four
 * were correct bindings for work that had not landed yet.
 *
 * That is not merely noisy, it is dangerous: the obvious response to "stale" is
 * to unbind, which strips governance from a peer's unmerged code, and it then
 * lands ungoverned and invisible to conformance. So a path is stale only when
 * NO ref that could still land holds it — checked against the local tree first
 * (cheapest, and the common case), then every remote branch.
 *
 * Returns the ref that keeps it alive so the report can say why, rather than
 * leaving the reader to rediscover it.
 */
export function pathHolder(rel, { exists, refs, inRef }) {
  if (exists(rel)) return "working tree";
  return refs().find((ref) => inRef(ref, rel)) ?? null;
}

/**
 * Whether a vanished bound path was split into a same-named module directory.
 *
 * The repository's own `rust-module-length` control tells agents to split any
 * module over 450 non-test lines, so `X.rs` becoming `X/mod.rs` plus siblings
 * is routine, expected, and breaks the binding every time: the binding still
 * names `X.rs`, which reports as stale, and every descendant reports as
 * unbound. Two controls in tension, with nothing connecting them.
 *
 * A split and a deletion need opposite responses — rebind the descendants
 * versus unbind the dead path — so reporting them identically sends half the
 * readers the wrong way.
 */
export function splitInto(rel, { listDir }) {
  const asDirectory = rel.replace(/\.rs$/, "");
  if (asDirectory === rel) return [];
  return listDir(asDirectory)
    .filter((name) => name.endsWith(".rs"))
    .map((name) => `${asDirectory}/${name}`)
    .sort();
}

/**
 * The bind/unbind calls that move a split binding onto the files it became.
 *
 * Pure: it decides, and the caller performs. That split is the point — the
 * decision is the part worth testing, and it must be inspectable before any
 * write reaches a repository-shared ledger.
 *
 * The repair is fully determined, which is what makes automating it safe at
 * all. Four separate occurrences are recorded in
 * `gaps.d/the-engine-was-ungoverned-and-the-gate-that-would-enforce-it.md`, and
 * every one was resolved the same way by hand: bind the descendants to the goal
 * their predecessor held, then unbind the dead path. Nothing was ever judged.
 * That fragment calls the recurrence "a standing tax on every module split",
 * and names the missing fix as a binding following a file when it moves.
 *
 * Deliberately narrow:
 * - Only a split is repaired. A genuine deletion has no successor to guess at,
 *   and unbinding it is a judgement about whether the governance was meant to
 *   end — not a mechanical consequence of a file moving.
 * - The goal and the mode carry across unchanged. Re-deciding either would make
 *   this a governance change wearing a cleanup's clothes.
 * - A descendant already bound to the same goal is skipped rather than bound
 *   again, so a half-repaired split converges instead of accumulating. The
 *   fourth recorded occurrence was exactly this shape: the descendants were
 *   already bound and only the dead row remained.
 *
 * `retire: false` binds the descendants and leaves the old path bound. That is
 * the split-but-still-in-flight case: this worktree split the file, another
 * branch still has it whole, and both facts are true at once. Binding the
 * descendants governs the new code immediately; keeping the old binding leaves
 * the peer's unmerged file governed. Over-governing for as long as both exist
 * is the safe direction, and the old path retires on its own once that branch
 * lands or goes.
 */
export function repairPlan(split, existingBindings) {
  const already = new Set(
    (existingBindings ?? []).map(
      (binding) => `${binding.goal_id}\u0000${binding.node_id}`,
    ),
  );
  const steps = [];
  for (const { goal_id, mode, node_id, descendants, retire = true } of split) {
    for (const file of descendants) {
      const target = `artifact:${file}`;
      if (already.has(`${goal_id}\u0000${target}`)) continue;
      steps.push({ action: "bind", goal_id, node_id: target, mode });
    }
    // Last, and per binding: the dead path is removed only after its successors
    // hold the governance, so an interrupted repair leaves the code
    // over-governed rather than ungoverned.
    if (retire) steps.push({ action: "unbind", goal_id, node_id });
  }
  return steps;
}

/** One line per repair step, in the vocabulary the reader can verify by hand. */
export function describeRepair(steps) {
  return steps.map((step) =>
    step.action === "bind"
      ? `  bind    ${step.node_id}  ->  ${step.goal_id} (${step.mode})`
      : `  unbind  ${step.node_id}  from  ${step.goal_id}`,
  );
}

/**
 * Perform a repair plan through Lodestar's own tool surface.
 *
 * Deliberately not SQL. This script opens `spec.db` read-only to audit it, and
 * that asymmetry is the design: a reader can afford to know the schema, a
 * writer cannot. `constitution_define` is where a binding is defined, and a
 * second writer would be a second definition of what a binding is — the two
 * would drift the first time the plane added a column or a rule.
 *
 * Stops at the first failure and reports how far it got, rather than pressing
 * on: the steps are ordered so that binds precede the unbind, so stopping early
 * leaves the code over-governed, which is the safe direction to fail in.
 */
export function applyRepair(
  repoRoot,
  steps,
  call = callTools,
  resolve = resolveServer,
) {
  const server = resolve(repoRoot, "lodestar");
  if (!server) {
    return { ok: false, done: 0, error: "no lodestar server binary found" };
  }
  let done = 0;
  for (const step of steps) {
    try {
      call(server, repoRoot, [
        {
          name: "constitution_define",
          arguments:
            step.action === "bind"
              ? {
                  action: "bind",
                  goal_id: step.goal_id,
                  node_ids: [step.node_id],
                  mode: step.mode,
                }
              : {
                  action: "unbind",
                  goal_id: step.goal_id,
                  node_ids: [step.node_id],
                },
        },
      ]);
      done += 1;
    } catch (error) {
      return { ok: false, done, error: String(error.message ?? error) };
    }
  }
  return { ok: true, done };
}

/** Binding rows from either the current or pre-rename Lodestar schema. */
export function goalArtifactBindings(db) {
  const tables = new Set(
    db
      .prepare(
        "select name from sqlite_master where type = 'table' and name in ('goal_artifacts', 'goal_code')",
      )
      .all()
      .map((table) => table.name),
  );
  if (tables.has("goal_artifacts")) {
    return db
      .prepare("select goal_id, node_id, mode from goal_artifacts")
      .all();
  }
  if (tables.has("goal_code")) {
    return db
      .prepare("select goal_id, node_id, 'governed' as mode from goal_code")
      .all();
  }
  throw new Error(
    "binding-audit: ledger has neither goal_artifacts nor legacy goal_code bindings; open it with a current Lodestar build to migrate it.",
  );
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
  const repair = argv.includes("--repair");
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
    const bindings = goalArtifactBindings(db);
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
    const git = (args) => {
      try {
        return execFileSync("git", args, {
          cwd: repoRoot,
          encoding: "utf8",
          stdio: ["ignore", "pipe", "ignore"],
        }).trim();
      } catch {
        return "";
      }
    };
    // Every remote branch, because any of them could still land and bring the
    // bound path with it. Computed once: this is one git call, not one per
    // binding.
    const remoteRefs = git([
      "for-each-ref",
      "--format=%(refname)",
      "refs/remotes",
    ])
      .split("\n")
      .map((ref) => ref.trim())
      .filter(Boolean);
    const holderOf = (rel) =>
      pathHolder(rel, {
        exists: (file) => fs.existsSync(path.join(repoRoot, file)),
        refs: () => remoteRefs,
        inRef: (ref, file) => {
          try {
            execFileSync("git", ["cat-file", "-e", `${ref}:${file}`], {
              cwd: repoRoot,
              stdio: "ignore",
            });
            return true;
          } catch {
            return false;
          }
        },
      });

    let stale = 0;
    let elsewhere = 0;
    let split = 0;
    const splitBindings = [];
    for (const binding of bindings) {
      if (!binding.node_id.startsWith("artifact:")) continue;
      const rel = binding.node_id.slice("artifact:".length);
      const holder = holderOf(rel);
      if (holder === "working tree") continue;
      const descendantsOf = (file) =>
        splitInto(file, {
          listDir: (dir) => {
            try {
              return fs.readdirSync(path.join(repoRoot, dir));
            } catch {
              return [];
            }
          },
        });
      if (holder) {
        // Alive on a branch that has not landed. Reporting this as stale is how
        // an agent gets talked into unbinding a peer's unmerged code.
        elsewhere += 1;
        console.log(`  IN FLIGHT  ${rel}   (${binding.goal_id})`);
        console.log(`             still present on ${holder} — not stale`);
        // A file this worktree split, which another branch still holds whole,
        // is both at once. The descendants need governing now; the old path
        // must keep it until that branch lands or goes. `retire: false` binds
        // without unbinding, which over-governs briefly rather than leaving
        // either side bare.
        const carried = descendantsOf(rel);
        if (carried.length > 0) {
          splitBindings.push({
            ...binding,
            descendants: carried,
            retire: false,
          });
          console.log(
            `             split here into ${carried.length} module(s); they will be bound without retiring this path`,
          );
        }
        continue;
      }
      const descendants = descendantsOf(rel);
      if (descendants.length > 0) {
        split += 1;
        splitBindings.push({ ...binding, descendants });
        console.log(`  SPLIT    ${rel}   (${binding.goal_id})`);
        console.log(
          `             became ${descendants.length} module(s); rebind them and unbind this path:`,
        );
        for (const file of descendants) console.log(`               ${file}`);
        continue;
      }
      stale += 1;
      console.log(`  MISSING  ${rel}   (${binding.goal_id})`);
    }
    if (stale + elsewhere + split === 0) console.log("  none");
    if (elsewhere > 0) {
      console.log(
        `\n  ${elsewhere} binding(s) name a path absent from this worktree but present on another\n` +
          "  branch. Bindings are repository-shared and paths are checkout-relative, so this\n" +
          "  is expected in a fleet. Unbinding them would strip governance from unmerged work.",
      );
    }
    const steps = repairPlan(splitBindings, bindings);
    if (steps.length > 0) {
      const retiring = splitBindings.filter((b) => b.retire !== false).length;
      const carrying = splitBindings.length - retiring;
      console.log(
        `\n  ${splitBindings.length} binding(s) name a file that became a module directory. The\n` +
          "  rust-module-length control asks for exactly this split, so it recurs; the repair carries\n" +
          `  the goal and mode across to the descendants${retiring > 0 ? ", and retires the dead path" : ""}` +
          `${carrying > 0 ? `\n  (${carrying} of them stay bound: another branch still holds the whole file)` : ""}:`,
      );
      for (const line of describeRepair(steps)) console.log(line);
      if (repair) {
        // Written through the Lodestar tool surface, never by SQL into
        // `spec.db`: the plane owns its store, and a second writer would be a
        // second definition of what a binding is.
        const applied = applyRepair(repoRoot, steps);
        console.log(
          applied.ok
            ? `\n  repaired: ${applied.done} step(s) applied.`
            : `\n  repair FAILED after ${applied.done} step(s): ${applied.error}\n` +
                "  Binds run before the unbind, so an interrupted repair leaves the code\n" +
                "  over-governed rather than ungoverned. Re-run to converge.",
        );
        if (!applied.ok) process.exitCode = 1;
      } else {
        console.log(
          "\n  Re-run with --repair to apply exactly the steps above.",
        );
      }
    }

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
      `\nsummary: ${unbound} unbound source files, ${stale} stale bindings, ` +
        `${split} split bindings, ${elsewhere} in flight on another branch, ` +
        `${stranded.length} stranded bindings (db ${dbPath})`,
    );
    // A split fails the check: it is a real binding defect with a known repair,
    // and leaving it green is how the descendants stay ungoverned. An in-flight
    // binding does not — it is correct, and only looks wrong from here.
    if (check && (unbound > 0 || stale > 0 || split > 0)) process.exitCode = 1;
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
