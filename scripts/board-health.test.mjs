// Tests for the board health report. Run with: make script-test
//
// The behaviour under test is one distinction: work a person can rule on
// versus work nobody can, both currently wearing the label `needs_human`.
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  classify,
  describe,
  describeGapsTriage,
  isLive,
  isStrandedClaim,
  mergedBranches,
  shippedButOpen,
} from "./board-health.mjs";
import { triageReport } from "./gaps.mjs";

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

/// Work that shipped and never closed. The board understating what is finished
/// is expensive in a way overstating it is not: `next_task` offers work that
/// already exists, and an agent rebuilds it. Seen repeatedly on this board.
test("a task whose branch has merged is named as shipped", () => {
  const merged = mergedBranches([
    "abc1234def5678 Merge pull request #186 from monk-eee/docs/the-blind-spot",
    "9990000aaa1111 Merge pull request #185 from monk-eee/fix/something-else",
  ]);
  const shipped = task("task:shipped", {
    status: "claimed",
    branch: "docs/the-blind-spot",
  });

  assert.equal(merged.get("docs/the-blind-spot"), "abc1234def5678");
  assert.equal(shippedButOpen(shipped, merged), true);

  const report = classify([{ task: shipped }], NOW, merged);
  assert.equal(report.shipped.length, 1);
  assert.equal(report.shipped[0].mergedAt, "abc1234def5678");
});

/// It reports and never closes: completing one would manufacture a receipt for
/// work the script did not witness, which ADR-0009 refuses.
test("shipped work is reported with its merge commit, not closed", () => {
  const merged = mergedBranches([
    "abc1234def5678 Merge pull request #186 from monk-eee/docs/the-blind-spot",
  ]);
  const shipped = task("task:shipped", {
    status: "claimed",
    branch: "docs/the-blind-spot",
  });
  const report = classify([{ task: shipped }], NOW, merged);
  const text = describe(report, [{ task: shipped }]);

  assert.match(text, /shipped, never closed  : 1/);
  assert.match(text, /abc1234d/);
  assert.match(text, /never closed: completing one here would manufacture/);
  assert.equal(shipped.status, "claimed", "the task must not be mutated");
});

/// A finished task is not "shipped and open" � it is simply finished, and
/// listing it would rebuild the inflated-backlog bug this report already fixed.
test("a task that already completed is not reported as shipped", () => {
  const merged = mergedBranches([
    "abc1234def5678 Merge pull request #186 from monk-eee/docs/the-blind-spot",
  ]);
  const done = task("task:done", {
    status: "done",
    branch: "docs/the-blind-spot",
  });

  assert.equal(shippedButOpen(done, merged), false);
  assert.equal(classify([{ task: done }], NOW, merged).shipped.length, 0);
});

/// A task on a branch that has not landed is ordinary work in progress.
test("an unmerged branch is not shipped", () => {
  const merged = mergedBranches([
    "abc1234def5678 Merge pull request #186 from monk-eee/docs/the-blind-spot",
  ]);
  const live = task("task:live", {
    status: "claimed",
    branch: "fleet/still-going",
  });

  assert.equal(shippedButOpen(live, merged), false);
});

/// A task claimed before the branch column existed records none, and must not
/// be guessed at � every task would otherwise match an empty branch name.
test("a task with no recorded branch is never shipped", () => {
  const merged = mergedBranches([
    "abc1234def5678 Merge pull request #186 from monk-eee/docs/the-blind-spot",
  ]);

  assert.equal(
    shippedButOpen(task("task:old", { status: "claimed" }), merged),
    false,
  );
  assert.equal(
    shippedButOpen(task("task:blank", { branch: "" }), merged),
    false,
  );
});

/// Branches are usually deleted on merge, so the ref is gone while the history
/// proving it landed is not. Anything that is not a pull-request merge � a
/// hand-made merge of main into a branch, say � names no branch and is skipped.
test("only pull-request merges name a branch", () => {
  const merged = mergedBranches([
    "1111111 Merge branch 'main' into fleet/whatever",
    "2222222 Merge remote-tracking branch 'origin/main'",
    "3333333 Merge pull request #1 from monk-eee/fix/real",
    "",
  ]);

  assert.deepEqual([...merged.keys()], ["fix/real"]);
});

/// Zero because nothing shipped unclosed, or zero because nothing records a
/// branch to check? Those read identically and mean opposite things. Reporting
/// a bare 0 while the answer is unknowable is the falsely-reassuring signal
/// this whole report exists to remove � and it is exactly what the first live
/// run produced, against a server too old to return the column.
test("a board with no recorded branches says unknown, not zero", () => {
  const merged = mergedBranches([
    "abc1234def5678 Merge pull request #186 from monk-eee/docs/the-blind-spot",
  ]);
  const entries = [{ task: task("task:old", { status: "claimed" }) }];

  const text = describe(classify(entries, NOW, merged), entries);

  assert.match(text, /shipped, never closed  : unknown/);
  assert.doesNotMatch(text, /shipped, never closed  : 0/);
});

/// And once any task does record one, the count is real and says so.
test("a board with recorded branches reports a real count", () => {
  const merged = mergedBranches([
    "abc1234def5678 Merge pull request #186 from monk-eee/docs/the-blind-spot",
  ]);
  const entries = [
    {
      task: task("task:live", {
        status: "claimed",
        branch: "fleet/still-going",
      }),
    },
  ];

  const text = describe(classify(entries, NOW, merged), entries);

  assert.match(text, /shipped, never closed  : 0/);
});

// --- Orphaned gaps: the same blind spot, for gaps.d instead of the board ----

/// An empty gaps.d is not a finding -- the report should say nothing rather
/// than announce "0 of 0 gaps are orphaned", which reads as a clean bill of
/// health for a directory that was simply never populated.
test("describeGapsTriage says nothing when there are no open gaps", () => {
  const triage = triageReport([], {}, Date.now());
  assert.equal(describeGapsTriage(triage), null);
});

/// The headline numbers, plus the orphaned rows themselves -- a person acting
/// on this report needs the names, not just the count.
test("describeGapsTriage reports the orphan count and names the orphans", () => {
  const gaps = [
    { name: "tracked.md", body: "- **Tracked.** fix is task:aaaaaaaaaaaa." },
    { name: "orphan.md", body: "- **Orphan.** no task yet." },
  ];
  const nowMs = 10 * 86_400_000;
  const firstSeen = {
    "gaps.d/tracked.md": 9 * 86_400,
    "gaps.d/orphan.md": 0,
  };

  const text = describeGapsTriage(triageReport(gaps, firstSeen, nowMs));

  assert.match(text, /gaps\.d fragments with no tracking task : 1 of 2/);
  assert.match(text, /oldest open gap {24}: 10 day\(s\)/);
  assert.match(text, /orphaned \(/);
  assert.match(text, /10d\s+orphan\.md/);
  assert.doesNotMatch(text, /tracked\.md/, "a tracked gap is not an orphan");
});

/// Every gap tracked by a task is the healthy state this report exists to
/// confirm, not just a silence to leave unexplained.
test("describeGapsTriage still reports totals when nothing is orphaned", () => {
  const gaps = [{ name: "tracked.md", body: "fix is task:aaaaaaaaaaaa." }];
  const text = describeGapsTriage(
    triageReport(gaps, { "gaps.d/tracked.md": 0 }, 86_400_000),
  );

  assert.match(text, /gaps\.d fragments with no tracking task : 0 of 1/);
  assert.doesNotMatch(text, /orphaned \(/, "nothing to list");
});
