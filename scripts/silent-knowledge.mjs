#!/usr/bin/env node
// Report recorded knowledge that can never be read.
//
// The conformance advisory matches knowledge on referenced nodes and nothing
// else, so a record whose evidence carries no `nodes` array is stored, counted,
// decayed — and structurally incapable of reaching any agent. The failure is
// silent in the one direction that matters: the record looks fine from the
// writer's side and simply never arrives.
//
// `record_knowledge` now warns at write time, which stops the population
// growing. This reports the ones already there, because a heap of 63 is not a
// backlog anybody can work: ranked by how much is at stake, it becomes a list.
//
// Usage:
//   node scripts/silent-knowledge.mjs [--db <path>] [--check] [--top <n>]
//
// --check exits 1 when any silent record remains, so this can gate CI once the
// backlog is cleared. It is deliberately not wired into CI yet: the repository
// is not clean, and a check that fails on day one gets switched off.

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";

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

/**
 * The nodes a record references, read the way the advisory reads them.
 *
 * Evidence is free-form JSON, so this must not assume it parses: a record whose
 * evidence is not JSON at all references nothing, which is exactly the silent
 * case rather than an error to throw on.
 */
export const referencedNodes = (evidence) => {
  if (!evidence) return [];
  let parsed;
  try {
    parsed = JSON.parse(evidence);
  } catch {
    return [];
  }
  const nodes = parsed?.nodes;
  return Array.isArray(nodes) ? nodes.filter((n) => typeof n === "string") : [];
};

/** A record can be read only if it names at least one node. */
export const isSilent = (record) =>
  referencedNodes(record.evidence).length === 0;

/**
 * Ranked so the list is workable. Length stands in for how much was invested in
 * writing it down, and a heavier, more recently confirmed record is one the
 * repository still believes — those are the ones worth rescuing first.
 */
export const rank = (records) =>
  [...records].sort(
    (a, b) =>
      b.weight - a.weight ||
      b.confirmed_at - a.confirmed_at ||
      (b.statement?.length ?? 0) - (a.statement?.length ?? 0),
  );

export const summarise = (records) => {
  const silent = records.filter(isSilent);
  return {
    total: records.length,
    silent: silent.length,
    share: records.length ? silent.length / records.length : 0,
    records: rank(silent),
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

  const report = summarise(records);
  const percent = (report.share * 100).toFixed(0);

  console.log(
    `silent-knowledge: ${report.silent} of ${report.total} records (${percent}%) can never be read`,
  );
  if (report.silent > 0) {
    console.log(
      "\nThe advisory matches on referenced nodes and nothing else. These name none,\n" +
        "so they are stored, counted, decayed, and can reach nobody.\n",
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

  if (check && report.silent > 0) process.exit(1);
}
