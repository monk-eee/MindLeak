// Tests for the silent-knowledge audit. Run with: node scripts/script-tests.mjs
//
// What this guards: the conformance advisory matches recorded knowledge on
// referenced nodes and nothing else, so a record naming none can never reach
// the agent it was written for. It is stored, counted, and unreachable — and
// nothing said so until this audit. Measured when it was written: 63 of 149
// active records were silent, including the lesson about skipping the ADR-0029
// pre-flight, which is precisely the lesson that would have prevented several
// of the day's drift verdicts.
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  isSilent,
  rank,
  referencedNodes,
  summarise,
} from "./silent-knowledge.mjs";

test("a record naming nodes is readable", () => {
  const evidence = JSON.stringify({ nodes: ["artifact:src/a.rs"] });
  assert.deepEqual(referencedNodes(evidence), ["artifact:src/a.rs"]);
  assert.equal(isSilent({ evidence }), false);
});

test("a record naming no nodes is silent", () => {
  // The overwhelmingly common shape: evidence written as prose or as an object
  // carrying anything except `nodes`.
  assert.deepEqual(referencedNodes(JSON.stringify({ method: "measured" })), []);
  assert.equal(isSilent({ evidence: JSON.stringify({ method: "x" }) }), true);
});

test("evidence that is not JSON references nothing rather than throwing", () => {
  // Evidence is free-form, so a record whose evidence never parsed is exactly
  // the silent case. Throwing here would make the audit fail on the very
  // records it exists to find.
  assert.deepEqual(referencedNodes("recorded while pairing"), []);
  assert.equal(isSilent({ evidence: "not json at all" }), true);
});

test("absent evidence is silent, not a crash", () => {
  assert.deepEqual(referencedNodes(undefined), []);
  assert.deepEqual(referencedNodes(""), []);
  assert.equal(isSilent({}), true);
});

test("an empty nodes array is silent, because the advisory has nothing to match", () => {
  assert.equal(isSilent({ evidence: JSON.stringify({ nodes: [] }) }), true);
});

test("non-string entries are not node ids", () => {
  // A malformed array must not read as coverage it does not have.
  const evidence = JSON.stringify({ nodes: [42, null, "artifact:src/a.rs"] });
  assert.deepEqual(referencedNodes(evidence), ["artifact:src/a.rs"]);
});

test("a nodes field that is not an array is not coverage", () => {
  assert.deepEqual(
    referencedNodes(JSON.stringify({ nodes: "artifact:a" })),
    [],
  );
});

test("the heaviest, most recently confirmed record is rescued first", () => {
  // Ranking is what makes 63 records workable rather than a heap. Weight leads,
  // because it is what the repository still believes.
  const records = [
    { statement: "old", weight: 0.4, confirmed_at: 900 },
    { statement: "believed", weight: 1.0, confirmed_at: 100 },
    { statement: "believed and fresh", weight: 1.0, confirmed_at: 500 },
  ];

  assert.deepEqual(
    rank(records).map((r) => r.statement),
    ["believed and fresh", "believed", "old"],
  );
});

test("ranking does not mutate its input", () => {
  const records = [
    { statement: "a", weight: 0.1, confirmed_at: 1 },
    { statement: "b", weight: 0.9, confirmed_at: 2 },
  ];
  rank(records);
  assert.deepEqual(
    records.map((r) => r.statement),
    ["a", "b"],
    "the caller's array is left alone",
  );
});

test("the summary counts and reports the share", () => {
  const loud = JSON.stringify({ nodes: ["artifact:a"] });
  const summary = summarise([
    { statement: "reachable", weight: 1, confirmed_at: 1, evidence: loud },
    { statement: "silent one", weight: 1, confirmed_at: 2, evidence: "{}" },
    { statement: "silent two", weight: 1, confirmed_at: 3, evidence: "{}" },
  ]);

  assert.equal(summary.total, 3);
  assert.equal(summary.silent, 2);
  assert.equal(Number(summary.share.toFixed(4)), 0.6667);
  assert.deepEqual(
    summary.records.map((r) => r.statement),
    ["silent two", "silent one"],
    "only the silent records are listed, ranked",
  );
});

test("an empty ledger reports no share rather than dividing by zero", () => {
  const summary = summarise([]);
  assert.equal(summary.total, 0);
  assert.equal(summary.silent, 0);
  assert.equal(summary.share, 0);
});
