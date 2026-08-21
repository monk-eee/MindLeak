// Tests for the constitution control-coverage report. Run with: make script-test
//
// The behaviour under test is one distinction: a clause with an active
// control that can reach its declared consequence, versus one that cannot --
// either because it has no active control at all, or because every control
// it has caps below what the clause declares (ADR-0034's ceiling rule).
import assert from "node:assert/strict";
import { test } from "node:test";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

import {
  classifyClause,
  coverageReport,
  normativeClauses,
  strongestCeiling,
} from "./control-coverage.mjs";

const goal = (id, over = {}) => ({
  id,
  title: `goal ${id}`,
  kind: "constraint",
  status: "active",
  consequence: "review",
  ...over,
});

const control = (ceiling, over = {}) => ({
  control_id: `control:${ceiling}`,
  ceiling,
  status: "active",
  ...over,
});

test("only active constraint/invariant goals are normative", () => {
  const goals = [
    goal("a", { kind: "constraint" }),
    goal("b", { kind: "invariant" }),
    goal("c", { kind: "objective" }),
    goal("d", { kind: "principle" }),
    goal("e", { kind: "constraint", status: "superseded" }),
  ];
  assert.deepEqual(
    normativeClauses(goals).map((g) => g.id),
    ["a", "b"],
  );
});

test("no controls at all has no ceiling", () => {
  assert.equal(strongestCeiling([]), null);
  assert.equal(strongestCeiling(undefined), null);
});

test("a retired control does not count toward the ceiling", () => {
  const controls = [control("block", { status: "retired" })];
  assert.equal(strongestCeiling(controls), null);
});

test("the ceiling is the strongest among several active controls", () => {
  const controls = [control("advise"), control("review"), control("block")];
  assert.equal(strongestCeiling(controls), "block");
});

test("a clause with zero active controls is reported uncovered", () => {
  const clause = goal("x", { consequence: "block" });
  const result = classifyClause(clause, []);
  assert.equal(result.gap, "no_active_control");
  assert.equal(result.declared, "block");
  assert.equal(result.ceiling, null);
});

test("a block clause behind only an observed/advisory control is weaker than declared", () => {
  const clause = goal("y", { consequence: "block" });
  const result = classifyClause(clause, [control("review")]);
  assert.equal(result.gap, "ceiling_below_declared");
  assert.equal(result.ceiling, "review");
});

test("a review clause behind a mechanical control is fully covered, not flagged for exceeding", () => {
  const clause = goal("z", { consequence: "review" });
  const result = classifyClause(clause, [control("block")]);
  assert.equal(result.gap, null);
});

test("a clause with no declared consequence defaults to review, matching SPEC-CONSTITUTION", () => {
  const clause = goal("w", { consequence: undefined });
  const result = classifyClause(clause, [control("review")]);
  assert.equal(result.declared, "review");
  assert.equal(result.gap, null);
});

test("coverageReport sorts uncovered clauses before weakened before fully covered", () => {
  const goals = [
    goal("covered", { consequence: "review" }),
    goal("uncovered", { consequence: "block" }),
    goal("weakened", { consequence: "block" }),
  ];
  const byId = new Map([
    ["covered", [control("review")]],
    ["uncovered", []],
    ["weakened", [control("review")]],
  ]);
  const report = coverageReport(goals, byId);
  assert.deepEqual(
    report.map((r) => r.clause_id),
    ["uncovered", "weakened", "covered"],
  );
});

// ---- CLI refusal paths (mirrors observe-module-length.test.mjs) -----------

const script = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "control-coverage.mjs",
);

const run = (env) =>
  spawnSync(process.execPath, [script], {
    encoding: "utf8",
    env: { ...process.env, ...env },
  });

test("a missing session is refused rather than reading anonymously", () => {
  const result = run({ LODESTAR_SESSION_ID: "" });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /LODESTAR_SESSION_ID/);
});

test("an unreachable Intent Plane fails loudly", () => {
  const result = run({
    LODESTAR_SESSION_ID: "0123456789abcdef0123456789abcdef",
    LODESTAR_MCP_BIN: path.join("no", "such", "binary"),
  });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /lodestar-mcp/);
});
