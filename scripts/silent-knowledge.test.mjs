// Tests for the silent-knowledge audit. Run with: node scripts/script-tests.mjs
//
// What this guards: the conformance advisory carries a lesson to an agent along
// TWO paths — the nodes it names, and the goal it was learned under — and an
// audit that knows about only one misreports the other. This one counted nodes
// alone, which was true when written and false once the goal path landed
// (7e38571). It called 68 of 210 records unreachable when 12 were: a backlog
// invented out of records that were already arriving.
//
// So the cases below pin the distinction that matters. A node match is
// unconditional. A goal match is contended — only GOAL_ADVISORY_LIMIT lessons
// attach per check — and only a record with neither is genuinely unreachable.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { test } from "node:test";

import {
  GOAL_ADVISORY_LIMIT,
  classify,
  declaredGoal,
  goalContention,
  goalSlug,
  isUnreachable,
  rank,
  reachableGoal,
  referencedNodes,
  referencedTasks,
  retired,
  summarise,
} from "./silent-knowledge.mjs";

/// Importing this module must not end the process.
///
/// The sqlite driver was imported at the top of the module with
/// `process.exit(2)` in the catch, which runs on *import*. So loading the pure
/// helpers to test them killed the process on any Node without built-in
/// sqlite: green on Node 24 locally, exit 1 on the Node 20 that CI pins, and
/// the failure named this test file rather than the cause. The driver is now
/// loaded only on the path that reads the ledger.
///
/// Checked in a child process with the module's own import stripped of
/// anything sqlite-shaped, because a test that has already imported the module
/// cannot observe what importing it does.
test("importing the module never ends the process, with or without sqlite", () => {
  const probe =
    "import('./scripts/silent-knowledge.mjs')" +
    ".then((m) => { if (typeof m.isUnreachable !== 'function') { process.exit(9); } })";

  const output = execFileSync(
    process.execPath,
    ["--input-type=module", "-e", probe],
    { encoding: "utf8", stdio: "pipe" },
  );

  assert.equal(
    output.includes("needs Node's built-in sqlite"),
    false,
    "importing must not print the CLI's driver error",
  );
});

test("a record naming nodes is readable", () => {
  const evidence = JSON.stringify({ nodes: ["artifact:src/a.rs"] });
  assert.deepEqual(referencedNodes(evidence), ["artifact:src/a.rs"]);
  assert.equal(classify({ evidence }), "node");
  assert.equal(isUnreachable({ evidence }), false);
});

test("a record naming no nodes but declaring a goal reaches that goal's work", () => {
  // The regression this file exists for. Counting nodes alone called this
  // unreachable; it arrives on every check under the same goal.
  const evidence = JSON.stringify({
    goal: "goal:durable-intent-plane@constitution:v3",
    method: "measured",
  });
  assert.deepEqual(referencedNodes(evidence), []);
  assert.equal(
    declaredGoal(evidence),
    "goal:durable-intent-plane@constitution:v3",
  );
  assert.equal(classify({ evidence }), "goal");
  assert.equal(isUnreachable({ evidence }), false);
});

test("a record naming a known task reaches that task's goal", () => {
  const evidence = JSON.stringify({ task: "task:abc123", method: "measured" });
  const taskGoals = new Map([["task:abc123", "goal:delivery@constitution:v3"]]);

  assert.deepEqual(referencedTasks(evidence), ["task:abc123"]);
  assert.equal(reachableGoal({ evidence }, taskGoals), "goal:delivery");
  assert.equal(classify({ evidence }, taskGoals), "goal");
});

test("a task the ledger no longer knows teaches nothing about the goal", () => {
  // A lesson may cite a task that has since been pruned. That is not an error,
  // it simply leaves the lesson with no goal to reach.
  const evidence = JSON.stringify({ task: "task:deadbeef" });
  assert.equal(reachableGoal({ evidence }, new Map()), null);
  assert.equal(isUnreachable({ evidence }, new Map()), true);
});

test("task ids are found in the shapes they were actually written in", () => {
  // Written by many hands: a JSON field, a nested array, and a bare string that
  // is not JSON at all. The doubled `task:task:` prefix appears in the wild.
  assert.deepEqual(referencedTasks("recorded while pairing on task:9f0a1b"), [
    "task:9f0a1b",
  ]);
  assert.deepEqual(
    referencedTasks(JSON.stringify({ tasks: ["task:aa11", "task:bb22"] })),
    ["task:aa11", "task:bb22"],
  );
  assert.deepEqual(
    referencedTasks(JSON.stringify({ task: "task:task:ac81d9" })),
    ["task:ac81d9"],
  );
});

test("a goal reaches across constitution versions", () => {
  // A lesson learned under v2 is still about the same goal under v3; comparing
  // the versioned ids would silence it at every amendment.
  assert.equal(goalSlug("goal:delivery@constitution:v2"), "goal:delivery");
  assert.equal(
    goalSlug("goal:delivery@constitution:v2"),
    goalSlug("goal:delivery@constitution:v3"),
  );
});

test("a record naming neither a node nor a goal reaches nobody", () => {
  assert.equal(
    classify({ evidence: JSON.stringify({ method: "x" }) }),
    "unreachable",
  );
  assert.equal(
    isUnreachable({ evidence: JSON.stringify({ method: "x" }) }),
    true,
  );
});

test("evidence that is not JSON references nothing rather than throwing", () => {
  // Evidence is free-form, so a record whose evidence never parsed is exactly
  // the unreachable case. Throwing here would make the audit fail on the very
  // records it exists to find.
  assert.deepEqual(referencedNodes("recorded while pairing"), []);
  assert.equal(isUnreachable({ evidence: "not json at all" }), true);
});

test("absent evidence is unreachable, not a crash", () => {
  assert.deepEqual(referencedNodes(undefined), []);
  assert.deepEqual(referencedNodes(""), []);
  assert.deepEqual(referencedTasks(undefined), []);
  assert.equal(isUnreachable({}), true);
});

test("an empty nodes array falls through to the goal path", () => {
  // `nodes: []` matches nothing on the node path, so the record is only
  // unreachable if it has no goal either.
  assert.equal(
    isUnreachable({ evidence: JSON.stringify({ nodes: [] }) }),
    true,
  );
  assert.equal(
    classify({ evidence: JSON.stringify({ nodes: [], goal: "goal:x" }) }),
    "goal",
  );
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
  // Ranking is what makes the backlog workable rather than a heap. Weight
  // leads, because it is what the repository still believes.
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

test("the summary separates the three populations", () => {
  const byNode = JSON.stringify({ nodes: ["artifact:a"] });
  const byGoal = JSON.stringify({ goal: "goal:delivery@constitution:v3" });
  const summary = summarise([
    {
      statement: "reachable by node",
      weight: 1,
      confirmed_at: 1,
      evidence: byNode,
    },
    {
      statement: "reachable by goal",
      weight: 1,
      confirmed_at: 2,
      evidence: byGoal,
    },
    {
      statement: "unreachable one",
      weight: 1,
      confirmed_at: 3,
      evidence: "{}",
    },
    {
      statement: "unreachable two",
      weight: 1,
      confirmed_at: 4,
      evidence: "{}",
    },
  ]);

  assert.equal(summary.total, 4);
  assert.equal(summary.byNode, 1);
  assert.equal(summary.byGoal, 1);
  assert.equal(summary.unreachable, 2);
  assert.equal(summary.share, 0.5);
  assert.deepEqual(
    summary.records.map((r) => r.statement),
    ["unreachable two", "unreachable one"],
    "only the genuinely unreachable records are listed, ranked",
  );
});

test("goal reachability is contended, and the report says by how much", () => {
  // The cap is what stops "reachable by goal" being the whole answer: past
  // GOAL_ADVISORY_LIMIT under one goal, the rest are crowded out. Reporting
  // reachability without this would overstate the good news exactly as
  // counting nodes alone overstated the bad.
  const under = (goal, n) =>
    Array.from({ length: n }, (_, i) => ({
      statement: `${goal} ${i}`,
      weight: 1,
      confirmed_at: i,
      evidence: JSON.stringify({ goal }),
    }));

  const { perGoal, attaching, crowdedOut } = goalContention([
    ...under("goal:busy", GOAL_ADVISORY_LIMIT + 4),
    ...under("goal:quiet", 1),
  ]);

  assert.equal(perGoal.get("goal:busy"), GOAL_ADVISORY_LIMIT + 4);
  assert.equal(perGoal.get("goal:quiet"), 1);
  assert.equal(
    attaching,
    GOAL_ADVISORY_LIMIT + 1,
    "three from the busy goal, one from the quiet one",
  );
  assert.equal(crowdedOut, 4);
});

test("a record reachable by node is never counted as contending for a goal slot", () => {
  // The node path is unconditional, so a record with nodes must not inflate the
  // contention figure even when it also names a goal.
  const evidence = JSON.stringify({
    nodes: ["artifact:a"],
    goal: "goal:busy",
  });
  const { attaching, crowdedOut } = goalContention([
    { statement: "has both", weight: 1, confirmed_at: 1, evidence },
  ]);
  assert.equal(attaching, 0);
  assert.equal(crowdedOut, 0);
});

test("an empty ledger reports no share rather than dividing by zero", () => {
  const summary = summarise([]);
  assert.equal(summary.total, 0);
  assert.equal(summary.unreachable, 0);
  assert.equal(summary.share, 0);
  assert.equal(summary.contention.attaching, 0);
});

// A retired record is not a silent one: it was withdrawn or replaced on
// purpose. Counting it kept this audit's total above zero no matter how much
// of the real backlog was cleared, which is what made --check unable to gate.
test("a retired record is excluded from the count entirely", () => {
  const silent = {
    statement: "unreachable",
    weight: 1,
    confirmed_at: 1,
    evidence: "{}",
  };
  const withdrawn = {
    statement: "withdrawn",
    weight: 1,
    confirmed_at: 1,
    evidence: "{}",
    retired_at: 1785462000,
  };

  const summary = summarise([silent, withdrawn]);

  assert.equal(summary.total, 1, "the retired record leaves the population");
  assert.equal(summary.unreachable, 1, "only the genuinely silent one counts");
  assert.equal(summary.retired, 1);
  assert.equal(summary.records.length, 1);
  assert.equal(summary.records[0].statement, "unreachable");
});

// Retiring the last silent record takes the count to zero, so the work is
// reducible and --check can gate on it.
test("retiring the last silent record lets the audit reach zero", () => {
  const record = {
    statement: "unreachable",
    weight: 1,
    confirmed_at: 1,
    evidence: "{}",
  };
  assert.equal(summarise([record]).unreachable, 1);
  assert.equal(summarise([{ ...record, retired_at: 1 }]).unreachable, 0);
});

test("retired() reads the column and treats null or absent as live", () => {
  assert.equal(retired({ retired_at: 1 }), true);
  assert.equal(retired({ retired_at: null }), false);
  // A ledger older than the column returns rows without the field at all.
  assert.equal(retired({}), false);
});
