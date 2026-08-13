// Tests for the delivery queue. Run with: make script-test
//
// The invariant under test is the whole point of the thing: exactly one branch
// updates at a time. Every other behaviour exists to stop that invariant from
// wedging the queue.
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  checksFailing,
  describe,
  effectiveMergeState,
  isQueued,
  nextAction,
  queueOrder,
} from "./delivery-queue.mjs";

const NOW = Date.parse("2026-07-28T12:00:00Z");
const ago = (mins) => new Date(NOW - mins * 60_000).toISOString();

const check = (status, conclusion = null) => ({
  name: "CI",
  status,
  conclusion,
});

const pr = (number, over = {}) => ({
  number,
  title: `pr ${number}`,
  headRefName: `branch-${number}`,
  createdAt: ago(120),
  updatedAt: ago(1),
  mergeStateStatus: "BEHIND",
  autoMergeRequest: { enabledAt: ago(60) },
  statusCheckRollup: [check("COMPLETED", "SUCCESS")],
  ...over,
});

test("an unarmed pull request is not in the queue", () => {
  assert.equal(isQueued(pr(1, { autoMergeRequest: null })), false);
  assert.equal(isQueued(pr(1)), true);

  const action = nextAction([pr(1, { autoMergeRequest: null })], NOW);
  assert.equal(action.kind, "idle");
  assert.equal(action.queued.length, 0);
});

/// Ordering by arming rather than by opening is what makes the queue fair: a
/// long-lived branch finished last must not jump work that was ready first.
test("the queue is first-in-first-out by when the author armed it, not when it was opened", () => {
  const oldButArmedLate = pr(1, {
    createdAt: ago(500),
    autoMergeRequest: { enabledAt: ago(5) },
  });
  const newButArmedFirst = pr(2, {
    createdAt: ago(30),
    autoMergeRequest: { enabledAt: ago(90) },
  });

  assert.deepEqual(
    queueOrder([oldButArmedLate, newButArmedFirst]).map((p) => p.number),
    [2, 1],
  );
});

/// The invariant. Updating a second branch while the first is still building
/// invalidates the first before it can land -- which is precisely the race the
/// queue exists to remove, reintroduced by the queue itself.
test("only one branch updates at a time", () => {
  const landing = pr(1, {
    mergeStateStatus: "BLOCKED",
    statusCheckRollup: [check("IN_PROGRESS")],
  });
  const waiting = pr(2);

  const action = nextAction([landing, waiting], NOW);
  assert.equal(action.kind, "wait");
  assert.equal(action.pr.number, 1);
  assert.match(action.reason, /about to land/);
});

/// Found by watching it run against thirteen real pull requests: it sat waiting
/// on a branch that was still BEHIND, and a branch that is behind cannot merge
/// no matter what its checks are doing. Its checks were running because its
/// author had just pushed, which has nothing to do with the queue. In a fleet
/// of ten agents pushing all day something is always running, so waiting on
/// "anything busy" starves the queue completely -- it would never take a single
/// turn. Only a branch that is already up to date is worth waiting for.
test("a branch that is still behind does not hold the queue, however busy it looks", () => {
  const busyButBehind = pr(1, {
    mergeStateStatus: "BEHIND",
    statusCheckRollup: [check("IN_PROGRESS")],
  });
  const idleAndBehind = pr(2);

  const action = nextAction([busyButBehind, idleAndBehind], NOW);
  assert.equal(
    action.kind,
    "update",
    "the queue must take a turn rather than starve",
  );
  assert.equal(
    action.pr.number,
    2,
    "the busy branch is skipped, not waited on",
  );
});

/// Without this the queue deadlocks on a single check that never reports, and
/// the failure mode is invisible: everything simply stops merging.
test("a check that never reports stops holding the queue after the stall threshold", () => {
  const wedged = pr(1, {
    mergeStateStatus: "BLOCKED",
    statusCheckRollup: [check("IN_PROGRESS")],
    updatedAt: ago(90),
  });
  const waiting = pr(2);

  const stillWaiting = nextAction([wedged, waiting], NOW, {
    stalledAfterMs: 120 * 60_000,
  });
  assert.equal(stillWaiting.kind, "wait");

  const movedOn = nextAction([wedged, waiting], NOW, {
    stalledAfterMs: 45 * 60_000,
  });
  assert.equal(movedOn.kind, "update");
  assert.equal(
    movedOn.pr.number,
    2,
    "the wedged entry is skipped, not retried forever",
  );
});

/// The sibling of the stalled check, and the more dangerous one because it is
/// silent. A check that never *starts* leaves an empty rollup, and an empty
/// rollup satisfies "nothing running" and "nothing failing" exactly as a fully
/// green one does -- so the branch reads as up to date and idle, the tick
/// returns wait, and no threshold ever ages it out. Observed live on 2026-07-30:
/// two armed pull requests sat with no run against their head after a
/// branch update, and the queue reported "waiting on GitHub to merge it" for
/// both. That time CI fired a few minutes later; had it not, one pull request
/// whose workflow never triggered would have held every branch behind it
/// indefinitely, and the log would have read like a healthy queue throughout.
test("a branch whose checks never started stops holding the queue after the stall threshold", () => {
  const silent = pr(1, {
    mergeStateStatus: "BLOCKED",
    statusCheckRollup: [],
    updatedAt: ago(90),
  });
  const waiting = pr(2);

  const stillWaiting = nextAction([silent, waiting], NOW, {
    stalledAfterMs: 120 * 60_000,
  });
  assert.equal(
    stillWaiting.kind,
    "wait",
    "a run can take minutes to appear, so a young branch is still worth waiting for",
  );

  const movedOn = nextAction([silent, waiting], NOW, {
    stalledAfterMs: 45 * 60_000,
  });
  assert.equal(
    movedOn.kind,
    "update",
    "past the threshold the queue must take a turn rather than wait forever",
  );
  assert.equal(
    movedOn.pr.number,
    2,
    "the branch behind the silent one gets its turn",
  );
});

/// "Waiting on GitHub to merge it" and "no workflow ever ran" are the same line
/// today, which is why the wedge above is invisible while it happens. The queue
/// cannot start the run itself -- but it can stop the state from reading as
/// healthy.
test("the tick names a branch whose checks never started", () => {
  const silent = pr(1, {
    mergeStateStatus: "BLOCKED",
    statusCheckRollup: [],
    updatedAt: ago(90),
  });

  const rendered = describe(nextAction([silent, pr(2)], NOW));
  assert.match(
    rendered,
    /no check has reported/,
    `the silent branch must be named, not left looking healthy:\n${rendered}`,
  );
});

/// A branch git cannot merge cleanly needs reconciling in its own worktree
/// (ADR-0038). Holding the whole queue behind it would let one conflict stop
/// every other delivery.
test("a conflicting branch is reported and stepped over, not waited on", () => {
  const conflicted = pr(1, { mergeStateStatus: "DIRTY" });
  const fine = pr(2);

  const action = nextAction([conflicted, fine], NOW);
  assert.equal(action.kind, "update");
  assert.equal(action.pr.number, 2);
  assert.deepEqual(
    action.blocked.map((p) => p.number),
    [1],
  );
  assert.match(describe(action), /#1 has a real conflict/);
});

/// A red branch is the author's problem. It must not be updated (that just
/// burns CI to fail again) and must not block the queue.
test("a failing branch is reported and stepped over", () => {
  const red = pr(1, {
    mergeStateStatus: "BLOCKED",
    statusCheckRollup: [check("COMPLETED", "FAILURE")],
  });
  const green = pr(2);

  assert.equal(checksFailing(red).length, 1);
  const action = nextAction([red, green], NOW);
  assert.equal(action.kind, "update");
  assert.equal(action.pr.number, 2);
  assert.deepEqual(
    action.failing.map((p) => p.number),
    [1],
  );
});

/// An up-to-date green branch is GitHub's to merge. The queue must not treat it
/// as work, or it will spin trying to update a branch that is already current.
test("an up-to-date branch is left for GitHub to merge", () => {
  const ready = pr(1, { mergeStateStatus: "BLOCKED" });
  const action = nextAction([ready], NOW);
  assert.equal(action.kind, "wait");
  assert.match(action.reason, /waiting on GitHub to merge it/);
});

/// Measured on the live queue: immediately after every merge, GitHub recomputes
/// mergeability and each entry reads UNKNOWN for a few seconds. That looked
/// identical to a quiet queue, so the tick did nothing and slept a full minute
/// -- once per merge, on every delivery. Naming the state is what lets the
/// watcher come back in seconds instead, and it is safe to come back early
/// precisely because a settling tick has by construction done nothing.
test("a queue mid-recompute is settling, not idle", () => {
  const recomputing = [
    pr(1, { mergeStateStatus: "UNKNOWN" }),
    pr(2, { mergeStateStatus: "UNKNOWN" }),
  ];
  const action = nextAction(recomputing, NOW);
  assert.equal(action.kind, "settling");
  assert.match(action.reason, /recomputing/);
  assert.match(describe(action), /looking again shortly/);
});

/// The distinction has to be narrow: one resolved entry means GitHub has an
/// answer, and the queue must act on it rather than sit in a settling loop.
test("a single resolved entry is enough to stop settling and take a turn", () => {
  const mixed = [
    pr(1, { mergeStateStatus: "UNKNOWN" }),
    pr(2, { mergeStateStatus: "BEHIND" }),
  ];
  const action = nextAction(mixed, NOW);
  assert.equal(action.kind, "update");
  assert.equal(action.pr.number, 2);
});

/// A branch that is current and green is GitHub's to merge, not ours to touch.
/// Either way there is nothing for the queue to update -- the distinction is
/// only in what it says it is doing.
test("a queue with nothing behind has no turn to take", () => {
  const action = nextAction([pr(1, { mergeStateStatus: "CLEAN" })], NOW);
  assert.notEqual(
    action.kind,
    "update",
    "there is nothing to bring up to date",
  );
  assert.match(action.reason, /merge it|no armed branch is behind/);
});

/// The reason the queue chose what it chose has to be legible, or the first
/// time it does something surprising nobody can tell whether it was right.
test("the rendered tick names the decision and every entry", () => {
  const rendered = describe(nextAction([pr(1), pr(2)], NOW));
  assert.match(rendered, /queue: 2 armed/);
  assert.match(rendered, /#1/);
  assert.match(rendered, /#2/);
  assert.match(rendered, /-> updating #1 from main/);
});

/// An unarmed pull request is not late in the queue, it is absent from it, and
/// reporting only the armed ones makes those two states look identical. Three
/// of five open pull requests were invisible here on the day this was written,
/// which reads as an empty backlog and is why "the queue never merges the old
/// ones" looked like an ordering bug when nothing was ever in the queue at all.
test("the tick names the open work the queue is not managing", () => {
  const action = nextAction([pr(1), pr(2, { autoMergeRequest: null })], NOW);

  assert.deepEqual(
    action.unqueued.map((p) => p.number),
    [2],
    "an unarmed pull request must be reported, not dropped",
  );
  assert.deepEqual(
    action.queued.map((p) => p.number),
    [1],
    "and it must not be treated as queued",
  );

  const rendered = describe(action);
  assert.match(rendered, /#2\s+\S+\s+not queued: nobody armed it/);
});

/// Silence about unmanaged work is the failure being fixed, so the absence of
/// any unarmed pull request must not print a phantom entry either.
test("a fully armed queue reports nothing as unmanaged", () => {
  const rendered = describe(nextAction([pr(1), pr(2)], NOW));

  assert.equal(rendered.includes("not queued"), false);
});

/// gaps.d/the-delivery-queue-trusts-a-stale-conflict-verdict.md: GitHub's
/// mergeStateStatus is cached and can still read DIRTY after main has moved on
/// and the branch would in fact merge cleanly. Without a verifyDirty predicate
/// the queue must keep trusting DIRTY exactly as it always did -- this is the
/// existing "conflicting branch" test, unchanged, proving the default is a
/// no-op.
test("with no verifyDirty supplied, a DIRTY verdict is trusted as before", () => {
  const conflicted = pr(1, { mergeStateStatus: "DIRTY" });
  const fine = pr(2);

  const action = nextAction([conflicted, fine], NOW);
  assert.equal(action.kind, "update");
  assert.equal(action.pr.number, 2);
  assert.deepEqual(
    action.blocked.map((p) => p.number),
    [1],
  );
});

/// The corrected case: verifyDirty reports the branch actually merges
/// cleanly, so the queue must stop reporting it as blocked and instead treat
/// it as an ordinary BEHIND branch it can update -- the fix this task exists
/// for.
test("a DIRTY verdict verifyDirty disproves is corrected to BEHIND and updated", () => {
  const staleVerdict = pr(1, { mergeStateStatus: "DIRTY" });
  const verifyDirty = (candidate) => {
    assert.equal(candidate.number, 1, "only the DIRTY branch should be asked");
    return false; // merges cleanly; the cached DIRTY was stale
  };

  const action = nextAction([staleVerdict], NOW, { verifyDirty });
  assert.equal(action.kind, "update");
  assert.equal(action.pr.number, 1);
  assert.deepEqual(
    action.blocked.map((p) => p.number),
    [],
    "a disproved conflict must not still be reported as blocked",
  );
  assert.equal(action.queued[0].mergeStateStatus, "BEHIND");
});

/// A genuine conflict must still block, or the fix would trade one false
/// negative for another: verifyDirty confirming DIRTY must leave the existing
/// hand-back behaviour exactly as it was.
test("a DIRTY verdict verifyDirty confirms still blocks", () => {
  const realConflict = pr(1, { mergeStateStatus: "DIRTY" });
  const fine = pr(2);
  const verifyDirty = () => true; // genuinely conflicts

  const action = nextAction([realConflict, fine], NOW, { verifyDirty });
  assert.equal(action.kind, "update");
  assert.equal(action.pr.number, 2);
  assert.deepEqual(
    action.blocked.map((p) => p.number),
    [1],
  );
  assert.match(describe(action), /#1 has a real conflict/);
});

test("effectiveMergeState leaves every non-DIRTY status untouched", () => {
  for (const status of ["BEHIND", "BLOCKED", "CLEAN", "HAS_HOOKS", "UNKNOWN"]) {
    assert.equal(
      effectiveMergeState(pr(1, { mergeStateStatus: status }), () => true),
      status,
    );
  }
});
