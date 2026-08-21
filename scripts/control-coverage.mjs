#!/usr/bin/env node
// Report which normative clauses have no active control that can actually
// reach their declared consequence.
//
// ADR-0034's ceiling rule means a clause is only as strong as the mechanism
// behind it: `effective = min(clause.consequence, control.power.ceiling())`.
// A constraint or invariant with zero active controls is not enforced at all
// -- it resolves at `advise` regardless of what it declares, because "a rule
// with no mechanism behind it is a preference" (controls/mod.rs). A clause
// that declares `block` but is backed only by an `observed`/`advisory`
// control (a ratchet, a self-reported hint) is not enforced as hard as it
// reads either -- it silently caps at `review`.
//
// Neither gap is visible from reading the constitution alone; both need the
// clause and its bound controls read together. This is that report. Like
// `board-health.mjs` and `binding-audit.mjs`, it only reports -- it binds no
// control and drafts no amendment, because deciding a clause's proportional
// consequence, or which mechanism should enforce it, is exactly the
// judgement ADR-0034 reserves for a human.
//
// There is deliberately no "failing longer than N days" metric here: control
// resolutions are not persisted anywhere queryable (only the current control
// definition and its baseline are stored), so a duration claim would have to
// be invented rather than measured. Reporting the coverage gap itself is the
// honest, available signal.
import { execFileSync } from "node:child_process";

import { callTools, resolveServer } from "./claim-gate.mjs";

const CONSEQUENCE_RANK = { advise: 0, review: 1, block: 2 };

/** Every active, normative (constraint/invariant) clause -- what conformance checks against. */
export function normativeClauses(goals) {
  return (goals ?? []).filter(
    (goal) =>
      goal.status === "active" &&
      (goal.kind === "constraint" || goal.kind === "invariant"),
  );
}

/**
 * The strongest ceiling any currently active control bound to a clause can
 * reach, or `null` when none of its controls are active (whether because it
 * has none at all, or every control it once had has been retired).
 */
export function strongestCeiling(controls) {
  const active = (controls ?? []).filter(
    (control) => control.status === "active",
  );
  if (active.length === 0) return null;
  return active.reduce((best, control) => {
    const rank = CONSEQUENCE_RANK[control.ceiling] ?? -1;
    return rank > CONSEQUENCE_RANK[best] ? control.ceiling : best;
  }, active[0].ceiling);
}

/**
 * One clause's coverage classification against the controls bound to it.
 *
 * `gap` is `null` for a clause whose active controls can reach at least its
 * declared consequence -- fully covered, nothing to report.
 */
export function classifyClause(clause, controls) {
  const declared = clause.consequence ?? "review";
  const ceiling = strongestCeiling(controls);
  if (ceiling === null) {
    return {
      clause_id: clause.id,
      title: clause.title,
      kind: clause.kind,
      declared,
      ceiling: null,
      gap: "no_active_control",
    };
  }
  if (CONSEQUENCE_RANK[ceiling] < CONSEQUENCE_RANK[declared]) {
    return {
      clause_id: clause.id,
      title: clause.title,
      kind: clause.kind,
      declared,
      ceiling,
      gap: "ceiling_below_declared",
    };
  }
  return {
    clause_id: clause.id,
    title: clause.title,
    kind: clause.kind,
    declared,
    ceiling,
    gap: null,
  };
}

/** Every normative clause's classification, worst gap first. */
export function coverageReport(goals, controlsByClauseId) {
  const order = { no_active_control: 0, ceiling_below_declared: 1, null: 2 };
  return normativeClauses(goals)
    .map((clause) => classifyClause(clause, controlsByClauseId.get(clause.id)))
    .sort((a, b) => order[a.gap] - order[b.gap]);
}

async function main() {
  const repoRoot = execFileSync("git", ["rev-parse", "--show-toplevel"], {
    encoding: "utf8",
  }).trim();

  const fail = (message) => {
    console.error(`control-coverage: ${message}`);
    process.exit(1);
  };

  const sessionId = process.env.LODESTAR_SESSION_ID || "";
  if (!/^[0-9a-f]{32}$/.test(sessionId)) {
    fail(
      "LODESTAR_SESSION_ID must be a 128-bit hex token; without a session the constitution cannot be read",
    );
  }
  const server = resolveServer(repoRoot, "lodestar");
  if (!server) {
    fail("no lodestar-mcp binary found; build one or set LODESTAR_MCP_BIN");
  }

  const [, goalsResult] = callTools(server, repoRoot, [
    { name: "open_session", arguments: { session_id: sessionId } },
    { name: "constitution_query", arguments: { action: "active" } },
  ]);
  if (!Array.isArray(goalsResult)) {
    fail(
      `constitution_query returned nothing usable: ${JSON.stringify(goalsResult)}`,
    );
  }

  const clauses = normativeClauses(goalsResult);
  const controlCalls = clauses.map((clause) => ({
    name: "clause_controls",
    arguments: { clause_id: clause.id },
  }));
  const results = callTools(server, repoRoot, [
    { name: "open_session", arguments: { session_id: sessionId } },
    ...controlCalls,
  ]);

  const controlsByClauseId = new Map();
  clauses.forEach((clause, index) => {
    const reply = results[index + 1];
    controlsByClauseId.set(clause.id, reply?.controls ?? []);
  });

  const report = coverageReport(goalsResult, controlsByClauseId);
  const uncovered = report.filter((row) => row.gap === "no_active_control");
  const weakened = report.filter((row) => row.gap === "ceiling_below_declared");
  const covered = report.filter((row) => row.gap === null);

  console.log(
    `control-coverage: ${clauses.length} normative clauses -- ${uncovered.length} with no active control, ` +
      `${weakened.length} weaker than declared, ${covered.length} fully covered`,
  );
  for (const row of uncovered) {
    console.log(
      `  NO CONTROL      ${row.declared.padEnd(7)} ${row.clause_id} -- ${row.title}`,
    );
  }
  for (const row of weakened) {
    console.log(
      `  WEAKER (${row.ceiling.padEnd(7)}) declares ${row.declared.padEnd(7)} ${row.clause_id} -- ${row.title}`,
    );
  }
}

if (process.argv[1] && process.argv[1].endsWith("control-coverage.mjs")) {
  main();
}
