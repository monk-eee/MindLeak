// Tests for the delivery queue. Run with: make script-test
//
// The invariant under test is the whole point of the thing: exactly one branch
// updates at a time. Every other behaviour exists to stop that invariant from
// wedging the queue.
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  checksFailing,
  describe,
  effectiveMergeState,
  isQueued,
  nextAction,
  queueOrder,
  readWatcherHeartbeat,
  sweepAnnouncement,
  sweepArgs,
  unattendedQueueNote,
  updateBranchMismatch,
  watcherIsRunning,
  WATCHER_STALE_MS,
  writeWatcherHeartbeat,
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

// --- verifying update-branch actually did what it claimed -------------------

/// gaps.d/update-branch-can-silently-drop-a-conflicts-losing-side.md: PR #507's
/// update-branch call exited 0 and reported no conflict, yet the resulting
/// tree silently dropped one side of a real three-way merge. Two genuinely
/// different tree hashes is the one case this must catch.
test("update-branch producing a different tree than the expected merge is a mismatch", () => {
  assert.equal(updateBranchMismatch("tree-a", "tree-b"), true);
});

/// The ordinary, overwhelmingly common case: the branch update produced
/// exactly the merge it was expected to.
test("update-branch producing the expected tree is not a mismatch", () => {
  assert.equal(updateBranchMismatch("tree-a", "tree-a"), false);
});

/// An unverifiable computation (a fetch failed, or the pre-check itself hit a
/// real conflict) must fall back to the old, unverified behaviour -- it must
/// never read as a false-positive mismatch just because one side is missing.
test("an unverifiable expected or actual tree is never reported as a mismatch", () => {
  assert.equal(updateBranchMismatch(null, "tree-a"), false);
  assert.equal(updateBranchMismatch("tree-a", null), false);
  assert.equal(updateBranchMismatch(null, null), false);
});

// --- the sweep runs in a fresh process ------------------------------------

test("the watcher never forces a sweep, and only deletes when applying", () => {
  // `--if-due` is what keeps the cadence the sweep persists; without it every
  // 60-second tick would force a full disk walk. `--apply` must track the
  // queue's own mode, or a --dry-run watcher deletes for real.
  assert.deepStrictEqual(sweepArgs(true, "S"), ["S", "--if-due", "--apply"]);
  assert.deepStrictEqual(sweepArgs(false, "S"), ["S", "--if-due"]);
});

test("delivery-queue does not import the rules that decide what is deleted", () => {
  // Regression, and the reason this task exists. Node loads a module once at
  // startup and never re-reads it, so an imported `sweepIfDue` in a process
  // that runs for days keeps deleting by the rules it booted with. Measured
  // 2026-08-13: a day-old watcher deleted the fleet host's node_modules that a
  // merged fix had taught the sweep to spare. Spawning is the fix; importing
  // again would silently reinstate the defect, so the import is what is banned.
  const source = readFileSync(
    fileURLToPath(new URL("./delivery-queue.mjs", import.meta.url)),
    "utf8",
  );
  const imports = source.match(/^import[\s\S]*?from\s+"[^"]+";$/gm) ?? [];
  assert.deepStrictEqual(
    imports.filter((line) => line.includes("artefact-sweep")),
    [],
  );
});

test("a repeated sweep refusal is said once, not on every tick", () => {
  // A refused sweep never records a run, so it refuses again every tick. Said
  // each time, a stale-checkout refusal is 60 identical lines an hour.
  const stale = "artefact-sweep: nothing done (stale checkout: X differs)";
  assert.equal(sweepAnnouncement(stale, null), stale);
  assert.equal(sweepAnnouncement(stale, stale), null);
});

test("a sweep result that changed is always said", () => {
  const first = "artefact-sweep: reclaimed 1.00 GiB across 2 directories";
  const second = "artefact-sweep: reclaimed 3.00 GiB across 4 directories";
  assert.equal(sweepAnnouncement(second, first), second);
});

test("a silent sweep says nothing at all", () => {
  // `--if-due` prints nothing when the sweep is not due, which is almost every
  // call. Whitespace must not read as news.
  assert.equal(sweepAnnouncement("", null), null);
  assert.equal(sweepAnnouncement("\n  \n", null), null);
});

// --- is anybody watching? --------------------------------------------------

const armedQueue = () => nextAction([pr(1), pr(2)], NOW);

/// The reason this exists. "Armed means finished" (ADR-0045, ADR-0062) rests on
/// something taking the turns, and a watcher nobody started looks exactly like
/// a watcher with nothing to do: silence. Measured over two days on this board,
/// armed pull requests sat BEHIND for hours because `make queue-watch` was
/// simply not running and nothing anywhere said so.
test("armed work waiting with no heartbeat at all says so, and says nothing has ever beaten here", () => {
  const note = unattendedQueueNote(armedQueue(), null, NOW);
  assert.match(note, /no delivery-queue watcher is running/);
  assert.match(note, /none has ever beaten here/);
  assert.match(note, /2 armed pull requests are waiting/);
  assert.match(note, /make queue-watch/);
});

/// A watcher that stopped and one that was never started need different things
/// done about them, so the note must not collapse them into one sentence.
test("a stale heartbeat says how long ago the watcher stopped", () => {
  const note = unattendedQueueNote(armedQueue(), NOW - 90 * 60_000, NOW);
  assert.match(note, /no delivery-queue watcher is running/);
  assert.match(note, /last beat 90m ago/);
});

/// The note has to be silent in the ordinary case or it stops being read.
test("a fresh heartbeat says nothing, because a watcher is taking the turns", () => {
  assert.equal(unattendedQueueNote(armedQueue(), NOW - 30_000, NOW), null);
});

/// An empty queue needs no watcher. Firing here would make the note appear on
/// every idle run, which is how a diagnostic becomes noise nobody reads.
test("nothing armed means nothing to say, however long the watcher has been down", () => {
  const idle = nextAction([pr(1, { autoMergeRequest: null })], NOW);
  assert.equal(idle.queued.length, 0);
  assert.equal(unattendedQueueNote(idle, null, NOW), null);
  assert.equal(unattendedQueueNote(idle, NOW - 10 * 60 * 60_000, NOW), null);
});

test("a heartbeat counts as running right up to the stale threshold, and not past it", () => {
  assert.equal(watcherIsRunning(NOW - WATCHER_STALE_MS + 1, NOW), true);
  assert.equal(watcherIsRunning(NOW - WATCHER_STALE_MS, NOW), false);
  // A clock that reads ahead is a clock problem, not evidence that nothing is
  // running -- a beat from the future must not report the watcher as stopped.
  assert.equal(watcherIsRunning(NOW + 60_000, NOW), true);
  assert.equal(watcherIsRunning(null, NOW), false);
  assert.equal(watcherIsRunning("1787700000000", NOW), false);
});

/// The one seam every pure test above assumes and none of them exercises: the
/// reader has to understand what the writer actually wrote. A shape mismatch
/// here would report "no watcher" forever while every other test stayed green.
test("a heartbeat the watcher wrote is a heartbeat the one-shot can read", () => {
  const dir = mkdtempSync(join(tmpdir(), "queue-heartbeat-"));
  try {
    writeWatcherHeartbeat(dir, NOW);
    assert.equal(readWatcherHeartbeat(dir), NOW);
    assert.equal(
      unattendedQueueNote(armedQueue(), readWatcherHeartbeat(dir), NOW),
      null,
    );

    // Absent and corrupt both read as "no watcher": the safe direction, since a
    // note nobody needed costs one line and a missing one costs a stalled queue.
    assert.equal(readWatcherHeartbeat(join(dir, "nowhere")), null);
    writeFileSync(
      join(dir, "delivery-queue-watcher.json"),
      "{not json",
      "utf8",
    );
    assert.equal(readWatcherHeartbeat(dir), null);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
