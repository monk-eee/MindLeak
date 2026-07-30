#!/usr/bin/env node
// Report recorded knowledge that cannot reach an agent.
//
// The conformance advisory consults learned knowledge along TWO paths, and an
// audit that knows about one of them misreports the other:
//
//   By node. When the evidence carries a `nodes` array and those ids intersect
//   the evidence's changed nodes, the lesson attaches. Unconditional: any task
//   touching those files sees it.
//
//   By goal. A lesson naming no node is not therefore anonymous. If its
//   evidence declares a `goal`, or names a task from which a goal is
//   reachable, it attaches to work under that same goal — but only the
//   strongest GOAL_ADVISORY_LIMIT of them do, so this path is contended.
//
// This audit counted the node path alone. That was correct when it was written
// and stopped being correct when the goal path landed (7e38571, "a lesson
// reaches the goal it was learned under"). It reported every node-less record
// as unreachable: 68 of 210 here, against 12 that genuinely are. Overstating by
// that much is not a rounding error — it invents a backlog, and the records it
// tells you to go and re-record are already arriving.
//
// So the report separates three populations, because they need different
// things done to them:
//   - reachable by node     — nothing to do
//   - reachable by goal     — arrives, but competes for a capped number of
//                             slots; naming nodes is how a lesson stops
//                             competing
//   - reachable by neither  — the real backlog
//
// Usage:
//   node scripts/silent-knowledge.mjs [--db <path>] [--check] [--top <n>]
//
// --check exits 1 when any record is reachable by neither path.

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";

/**
 * How many goal-matched lessons the advisory attaches to a single check.
 *
 * Mirrors GOAL_ADVISORY_LIMIT in
 * crates/lodestar-core/src/facade/conformance/verdict.rs. If that constant
 * moves and this does not, the contention figure is what goes quietly wrong —
 * the same shape of failure this rewrite exists to correct.
 */
export const GOAL_ADVISORY_LIMIT = 3;

/**
 * The sqlite driver, loaded only when this actually reads the ledger.
 *
 * It used to be imported at the top of the module, with `process.exit(2)` in
 * the catch. That runs on *import*, so merely importing the pure helpers to
 * test them killed the process on any Node without built-in sqlite — green on
 * Node 24 here, exit 1 on the Node 20 CI pins, and the failure named the test
 * file rather than the cause. A module that ends the process as a side effect
 * of being loaded cannot be tested, and the environment decides whether you
 * find out.
 */
const openLedger = async (dbPath) => {
  let DatabaseSync;
  try {
    ({ DatabaseSync } = await import("node:sqlite"));
  } catch {
    console.error(
      "silent-knowledge: this needs Node's built-in sqlite (Node 22.5+). Upgrade Node, or pass --db to a build that has it.",
    );
    process.exit(2);
  }
  return new DatabaseSync(dbPath, { readOnly: true });
};

const argv = process.argv.slice(2);
const check = argv.includes("--check");
const topFlag = argv.indexOf("--top");
const top =
  topFlag !== -1 && argv[topFlag + 1] ? Number(argv[topFlag + 1]) : 15;

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
  const dbFlag = argv.indexOf("--db");
  if (dbFlag !== -1 && argv[dbFlag + 1]) return argv[dbFlag + 1];
  const repositories = path.join(stateRoot(), "repositories");
  if (!fs.existsSync(repositories)) {
    console.error(
      `silent-knowledge: no repository state at ${repositories}. Pass --db <path to spec.db>; storage_status reports it.`,
    );
    process.exit(2);
  }
  const found = fs
    .readdirSync(repositories)
    .map((id) => path.join(repositories, id, "spec.db"))
    .filter((p) => fs.existsSync(p));
  if (found.length !== 1) {
    console.error(
      `silent-knowledge: expected exactly one spec.db under ${repositories}, found ${found.length}. Pass --db.`,
    );
    process.exit(2);
  }
  return found[0];
}

const parseEvidence = (evidence) => {
  if (!evidence) return null;
  try {
    return JSON.parse(evidence);
  } catch {
    // Evidence is free-form, so a record whose evidence never parsed simply
    // references nothing. Throwing here would make the audit fail on the very
    // records it exists to find.
    return null;
  }
};

/**
 * The nodes a record references, read the way the advisory reads them
 * (`{"nodes": [...]}`).
 */
export const referencedNodes = (evidence) => {
  const nodes = parseEvidence(evidence)?.nodes;
  return Array.isArray(nodes) ? nodes.filter((n) => typeof n === "string") : [];
};

/** The goal a record says it was learned under (`{"goal": "goal:..."}`). */
export const declaredGoal = (evidence) => {
  const goal = parseEvidence(evidence)?.goal;
  return typeof goal === "string" ? goal : null;
};

/**
 * Every task id named anywhere in the evidence.
 *
 * Scanned as text rather than parsed, mirroring the Rust deliberately: this
 * provenance was written by many hands and appears as a JSON field, inside
 * nested arrays, and as a bare `task:{id}` that is not JSON at all. A reader
 * understanding only one shape would silence the records written in the others
 * — and would then report them as a backlog to redo.
 */
export const referencedTasks = (evidence) => [
  ...new Set(
    [...String(evidence ?? "").matchAll(/task:([0-9a-fA-F]+)/g)].map(
      (match) => `task:${match[1]}`,
    ),
  ),
];

/**
 * A goal id without its constitution version, so a lesson learned under v2
 * still reaches work governed by v3. Mirrors `goal_slug` in the Rust.
 */
export const goalSlug = (goalId) => String(goalId ?? "").split("@")[0];

/**
 * The goal a lesson can reach, or null. Declared goal first, then the goal of
 * the first task it names that the ledger still knows — a lesson may cite a
 * task that has since been pruned, which is not an error, it simply teaches
 * nothing about the goal.
 *
 * `taskGoals` maps task id to goal id.
 */
export const reachableGoal = (record, taskGoals = new Map()) => {
  const declared = declaredGoal(record.evidence);
  if (declared) return goalSlug(declared);
  for (const id of referencedTasks(record.evidence)) {
    const goal = taskGoals.get(id);
    if (goal) return goalSlug(goal);
  }
  return null;
};

/**
 * Which path, if any, carries this record to an agent.
 *
 * `"node"` and `"goal"` are not equivalent: a node match is unconditional,
 * while a goal match competes for GOAL_ADVISORY_LIMIT slots against every other
 * lesson under the same goal.
 */
export const classify = (record, taskGoals = new Map()) => {
  if (referencedNodes(record.evidence).length > 0) return "node";
  return reachableGoal(record, taskGoals) ? "goal" : "unreachable";
};

/** A record reaches nobody at all: it names no node and no resolvable goal. */
export const isUnreachable = (record, taskGoals = new Map()) =>
  classify(record, taskGoals) === "unreachable";

/**
 * Ranked so the list is workable. Weight leads because it is what the
 * repository still believes; a heavier, more recently confirmed record is the
 * one worth rescuing first.
 */
export const rank = (records) =>
  [...records].sort(
    (a, b) =>
      b.weight - a.weight ||
      b.confirmed_at - a.confirmed_at ||
      (b.statement?.length ?? 0) - (a.statement?.length ?? 0),
  );

/**
 * How many goal-reachable lessons actually attach, and how many are crowded out
 * by the cap. Reported because "reachable by goal" on its own would overstate
 * the good news exactly as the old count overstated the bad.
 */
export const goalContention = (records, taskGoals = new Map()) => {
  const perGoal = new Map();
  for (const record of records) {
    if (classify(record, taskGoals) !== "goal") continue;
    const goal = reachableGoal(record, taskGoals);
    perGoal.set(goal, (perGoal.get(goal) ?? 0) + 1);
  }
  let attaching = 0;
  let crowdedOut = 0;
  for (const count of perGoal.values()) {
    const attach = Math.min(count, GOAL_ADVISORY_LIMIT);
    attaching += attach;
    crowdedOut += count - attach;
  }
  return { perGoal, attaching, crowdedOut };
};

export const summarise = (records, taskGoals = new Map()) => {
  const byNode = [];
  const byGoal = [];
  const unreachable = [];
  for (const record of records) {
    const where = classify(record, taskGoals);
    if (where === "node") byNode.push(record);
    else if (where === "goal") byGoal.push(record);
    else unreachable.push(record);
  }
  return {
    total: records.length,
    byNode: byNode.length,
    byGoal: byGoal.length,
    unreachable: unreachable.length,
    share: records.length ? unreachable.length / records.length : 0,
    contention: goalContention(records, taskGoals),
    records: rank(unreachable),
  };
};

if (import.meta.filename === process.argv[1]) {
  const dbPath = resolveDb();
  const db = await openLedger(dbPath);
  const records = db
    .prepare(
      "select id, statement, evidence, weight, half_life_hours, confirmed_at from knowledge",
    )
    .all();

  const taskGoals = new Map(
    db
      .prepare("select id, goal_id from tasks")
      .all()
      .map((task) => [task.id, task.goal_id]),
  );

  const report = summarise(records, taskGoals);
  const percent = (report.share * 100).toFixed(0);

  console.log(
    `silent-knowledge: ${report.unreachable} of ${report.total} records (${percent}%) can reach nobody`,
  );
  console.log(
    `  ${report.byNode} reach agents by the nodes they name;\n` +
      `  ${report.byGoal} name no node but reach work under the goal they were learned under,\n` +
      `    of which ${report.contention.attaching} can attach on any one check and ${report.contention.crowdedOut} are crowded\n` +
      `    out by the cap of ${GOAL_ADVISORY_LIMIT} per goal — naming nodes is how a lesson stops competing.`,
  );

  if (report.unreachable > 0) {
    console.log(
      "\nThese name no node, and no goal or task from which one is reachable, so\n" +
        "they are stored, counted, decayed, and arrive nowhere.\n",
    );
    for (const record of report.records.slice(0, top)) {
      const first = (record.statement ?? "").replace(/\s+/g, " ").slice(0, 110);
      console.log(`  ${record.id}  w=${record.weight.toFixed(2)}`);
      console.log(`      ${first}...`);
    }
    if (report.records.length > top) {
      console.log(
        `\n  ... and ${report.records.length - top} more (--top ${report.records.length} for all)`,
      );
    }
    console.log(
      "\nRepair: knowledge is append-only and nothing attaches nodes retrospectively,\n" +
        "so re-record the content with an evidence `nodes` array — after re-verifying it\n" +
        "is still true. Copying a stale claim forward is worse than leaving it silent.",
    );
  }

  if (check && report.unreachable > 0) process.exit(1);
}
