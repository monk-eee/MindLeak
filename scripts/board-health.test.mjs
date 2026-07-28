// Tests for the board health report. Run with: make script-test
//
// The behaviour under test is one distinction: work a person can rule on
// versus work nobody can, both currently wearing the label `needs_human`.
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  classify,
  describe,
  isLive,
  isStrandedClaim,
} from "./board-health.mjs";

const NOW = 1_800_000_000;

const task = (id, over = {}) => ({
  id,
  title: `task ${id}`,
  status: "in_review",
  ...over,
});

const audit = (findings, verdict = "needs_human") => ({ verdict, findings });

/// The first version's bug, caught before it merged and worth keeping honest.
/// A task keeps its conformance audits after it finishes, so classifying by
/// "latest audit" alone counted completed work as pending. The first live run
/// reported 51 parked tasks; every single one was already done or abandoned,
/// and the true figure was zero. Inflating a backlog sends people looking for
/// work that does not exist -- the same disease as the verdict this report was
/// written to untangle.
test("a finished task is history, not a backlog", () => {
  const entries = [
    {
      task: task("done-one", { status: "done" }),
      audit: audit("evidence contains no provenance-bearing mutation"),
    },
    {
      task: task("gone", { status: "abandoned" }),
      audit: audit("governed code changed without a covering task: goal:x"),
    },
    {
      task: task("live"),
      audit: audit("evidence contains no provenance-bearing mutation"),
    },
  ];

  assert.equal(isLive(entries[0].task), false);
  assert.equal(isLive(entries[2].task), true);

  const { unresolvable, decidable } = classify(entries, NOW);
  assert.deepEqual(
    unresolvable.map((e) => e.task.id),
    ["live"],
    "only work that is still open counts",
  );
  assert.equal(decidable.length, 0, "an abandoned task asks nothing of anyone");
});

/// A lapsed claim on a finished task is not stranded -- nobody needs to recover
/// work that already landed.
test("a terminal task is never reported as a stranded claim", () => {
  const finished = task("shipped", {
    status: "done",
    lease_expires_at: NOW - 60,
  });
  const held = task("held", { status: "claimed", lease_expires_at: NOW - 60 });

  const { stranded } = classify([{ task: finished }, { task: held }], NOW);
  assert.deepEqual(
    stranded.map((e) => e.task.id),
    ["held"],
  );
});

/// The finding this whole report exists for. An empty evidence bundle means the
/// work was never ingested, so there is nothing for a person to weigh -- but it
/// lands under the same verdict as a genuine judgement call, and on this
/// repository it outnumbered them four to one.
test("empty evidence is unresolvable, not undecided", () => {
  const entries = [
    {
      task: task("a"),
      audit: audit("evidence contains no provenance-bearing mutation"),
    },
    {
      task: task("b"),
      audit: audit("governed code changed without a covering task: goal:x"),
    },
  ];

  const { unresolvable, decidable } = classify(entries, NOW);
  assert.deepEqual(
    unresolvable.map((e) => e.task.id),
    ["a"],
  );
  assert.deepEqual(
    decidable.map((e) => e.task.id),
    ["b"],
  );
});

/// Any other verdict is somebody's business but not this report's. Sweeping
/// aligned or drift work in here would turn a signal into a list.
test("only needs_human is parked work", () => {
  const entries = [
    { task: task("a"), audit: audit("all good", "aligned") },
    { task: task("b"), audit: audit("something", "drift") },
    { task: task("c"), audit: undefined },
  ];

  const { unresolvable, decidable } = classify(entries, NOW);
  assert.equal(unresolvable.length, 0);
  assert.equal(decidable.length, 0);
});

/// A lapsed claim still holds scope against other agents (ADR-0048), so it has
/// to be visible even though it carries no verdict at all.
test("a claim whose lease lapsed is stranded, whatever its verdict", () => {
  const lapsed = task("a", { status: "claimed", lease_expires_at: NOW - 60 });
  const live = task("b", { status: "claimed", lease_expires_at: NOW + 600 });

  assert.equal(isStrandedClaim(lapsed, NOW), true);
  assert.equal(isStrandedClaim(live, NOW), false);

  const { stranded } = classify([{ task: lapsed }, { task: live }], NOW);
  assert.deepEqual(
    stranded.map((e) => e.task.id),
    ["a"],
  );
});

/// A claim with no lease recorded must not be reported as lapsed -- guessing
/// would put honest work on a list of problems.
test("a claim with no lease is not assumed stranded", () => {
  const noLease = task("a", { status: "claimed" });
  assert.equal(isStrandedClaim(noLease, NOW), false);
});

/// The ratio is the finding, so it has to be stated rather than left for the
/// reader to compute from two counts.
test("the report states what share of parked work nobody can resolve", () => {
  const entries = [
    {
      task: task("a"),
      audit: audit("evidence contains no provenance-bearing mutation"),
    },
    {
      task: task("b"),
      audit: audit("evidence contains no provenance-bearing mutation"),
    },
    {
      task: task("c"),
      audit: audit("evidence contains no provenance-bearing mutation"),
    },
    {
      task: task("d"),
      audit: audit("governed code changed without a covering task: goal:x"),
    },
  ];

  const rendered = describe(classify(entries, NOW), entries);
  assert.match(rendered, /needs a human decision : 1/);
  assert.match(rendered, /nobody can resolve     : 3/);
  assert.match(rendered, /75% of parked work is unresolvable/);
});

/// A clean board must not invent a percentage from nothing.
test("a board with nothing parked reports no ratio", () => {
  const rendered = describe(classify([], NOW), []);
  assert.match(rendered, /board: 0 tasks/);
  assert.doesNotMatch(rendered, /% of parked work/);
});
