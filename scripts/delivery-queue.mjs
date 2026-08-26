#!/usr/bin/env node
// The delivery queue: a merge queue we run ourselves.
//
// ADR-0061 decided that delivery is queued, not raced, and chose GitHub's merge
// queue to do it. That turned out to be unavailable here — the `merge_queue`
// ruleset rule is refused on this repository (a public repo owned by a user
// account rather than an organisation), while the same endpoint accepts other
// rules with the same credentials. The decision was right and the mechanism was
// not available, so this is the mechanism instead.
//
// WHAT IT DOES NOT DO, DELIBERATELY: it does not merge. Merging stays with
// GitHub's auto-merge, gated by the same five required checks and the same
// branch protection as before. An agent that merged directly would be a second
// path to `main` that protection does not govern, which is a bigger problem
// than the one being solved. This only decides *whose turn it is to update*.
//
// The contention it removes: with `strict` (require branches up to date) on and
// N armed pull requests, every merge makes the other N-1 stale, and each one
// that updates itself burns a full check run against a `main` that the next
// merge invalidates again. Left to itself that is O(N^2) check runs and a
// permanent traffic jam. Serialising the update step makes it O(N): exactly one
// branch is brought up to date at a time, it merges, and the next one starts
// from the `main` that actually resulted.
//
// Armed means queued (ADR-0045): a pull request with auto-merge enabled is one
// whose author has declared it finished. That is the queue's membership test,
// so nothing new has to be remembered or labelled.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

/** The sweep script beside this one, whatever checkout this is. */
const SWEEP_SCRIPT = fileURLToPath(
  new URL("./artefact-sweep.mjs", import.meta.url),
);

/**
 * How the watcher asks for a sweep.
 *
 * NOT an import. Node loads a module once, at startup, and never re-reads it,
 * but this process runs for days -- so an imported `sweepIfDue` keeps deleting
 * by the rules it booted with, and nothing in git can see that because the file
 * on disk is already correct. Measured 2026-08-13: a watcher a day old deleted
 * the fleet host's `editors/vscode/node_modules` that PR #435 had specifically
 * taught the sweep to spare, and PR #453's freshness guard could not help
 * either, because a process that never re-reads its source never reaches it.
 * A child process reads the rules at the moment it uses them, so the age of the
 * watcher stops being a variable.
 *
 * `--if-due` because the cadence is the sweep's to keep: forcing here would run
 * a disk walk every 60 seconds.
 */
export function sweepArgs(apply, script = SWEEP_SCRIPT) {
  return apply ? [script, "--if-due", "--apply"] : [script, "--if-due"];
}

/**
 * What to print for a sweep result, given what was printed last.
 *
 * A refused sweep never records a run, so it refuses again on every tick. Said
 * each time, a stale-checkout refusal is 60 identical lines an hour, which is
 * how a log becomes something nobody reads; said once, it is the one line that
 * gets it fixed. Returns null for nothing to say.
 */
export function sweepAnnouncement(output, previous) {
  const said = output.trim();
  return !said || said === previous ? null : said;
}

/** Fields the decision needs. Kept small so the core stays pure and testable. */
export const PR_FIELDS =
  "number,title,headRefName,createdAt,mergeStateStatus,autoMergeRequest,statusCheckRollup";

/** A check run that has not reported yet. */
const isRunning = (check) =>
  check.status === "IN_PROGRESS" ||
  check.status === "QUEUED" ||
  check.status === "PENDING";

/** A check run that reported something other than success. */
const isFailing = (check) =>
  check.status === "COMPLETED" &&
  check.conclusion &&
  check.conclusion !== "SUCCESS" &&
  check.conclusion !== "NEUTRAL" &&
  check.conclusion !== "SKIPPED";

export const checksRunning = (pr) =>
  (pr.statusCheckRollup ?? []).some(isRunning);
export const checksFailing = (pr) =>
  (pr.statusCheckRollup ?? []).filter(isFailing);

/**
 * A head no check run has reported against at all.
 *
 * This has to be its own question because an empty rollup answers "anything
 * running?" and "anything failing?" exactly as a fully green one does, so a
 * branch whose workflow never fired is otherwise indistinguishable from a
 * branch that passed everything.
 */
export const checksAbsent = (pr) => (pr.statusCheckRollup ?? []).length === 0;

/**
 * No verdict yet, for either reason: a check is still running, or none has
 * started. Both are worth waiting for, and both must age out.
 */
export const verdictPending = (pr) => checksRunning(pr) || checksAbsent(pr);

/** A pull request is in the queue when its author has armed it (ADR-0045). */
export const isQueued = (pr) => Boolean(pr.autoMergeRequest);

/**
 * Queue order is first-in-first-out by the moment the author armed it, falling
 * back to creation time when GitHub does not report an arming timestamp.
 * Ordering by arming rather than by opening is what makes the queue fair: a
 * long-lived draft that is finished last does not jump work that was ready
 * first.
 */
export function queueOrder(prs) {
  const at = (pr) =>
    Date.parse(pr.autoMergeRequest?.enabledAt ?? pr.createdAt ?? 0) || 0;
  return [...prs]
    .filter(isQueued)
    .sort((a, b) => at(a) - at(b) || a.number - b.number);
}

/** A branch that is up to date with the base is one that could merge now. */
export const isUpToDate = (pr) =>
  pr.mergeStateStatus === "BLOCKED" ||
  pr.mergeStateStatus === "CLEAN" ||
  pr.mergeStateStatus === "HAS_HOOKS";

/**
 * `mergeStateStatus` is a cached answer GitHub recomputes lazily, and after a
 * burst of merges it can keep reporting `DIRTY` for a branch that merges
 * cleanly (gaps.d records this happening to a real branch: `git merge-tree`
 * and the REST `mergeable` field both said clean while `gh pr list` still said
 * `DIRTY`). Trusting it directly makes the one case the queue is designed to
 * hand back -- a real conflict -- also the case it gets wrong, and wrong in a
 * way nothing ever rechecks: the same stale field is read every tick.
 *
 * `verifyDirty(pr)` is the correction, injected rather than hard-coded so the
 * pure decision stays git- and network-free in tests. Given no predicate, a
 * `DIRTY` verdict is trusted exactly as before -- this function changes
 * nothing unless a caller opts in. Given one that reports the branch actually
 * merges cleanly, the verdict is corrected to `BEHIND`, which is what a stale
 * `DIRTY` almost always really is: main moved on and GitHub has not caught up.
 */
export function effectiveMergeState(pr, verifyDirty) {
  if (pr.mergeStateStatus !== "DIRTY") return pr.mergeStateStatus;
  if (verifyDirty && !verifyDirty(pr)) return "BEHIND";
  return "DIRTY";
}

/**
 * Decide the single next thing to do.
 *
 * The invariant: do not update a branch while another branch is *about to
 * merge*, because that merge is what will make the update stale again.
 *
 * "About to merge" is narrower than "busy", and the difference matters. A
 * branch that is still behind the base cannot merge no matter what its checks
 * are doing — its checks are running because its author just pushed, which has
 * nothing to do with us. Waiting on those would starve the queue completely: in
 * a fleet of ten agents pushing all day, something is always running, so the
 * queue would never take a turn at all. Only a branch that is already up to
 * date and still resolving is worth waiting for.
 *
 * `stalledAfterMs` is the escape hatch: a check that never reports would
 * otherwise wedge the queue forever, so after that long we stop waiting on it
 * and let the next branch through.
 */
export function nextAction(
  prs,
  now,
  { stalledAfterMs = 45 * 60 * 1000, verifyDirty } = {},
) {
  // Corrected once, up front, so every downstream question (blocked?,
  // up to date?, behind?) sees one consistent verdict per branch rather than
  // each re-deriving its own opinion of what DIRTY really meant this tick.
  const queued = queueOrder(prs).map((pr) => ({
    ...pr,
    mergeStateStatus: effectiveMergeState(pr, verifyDirty),
  }));
  const blocked = queued.filter((pr) => pr.mergeStateStatus === "DIRTY");
  const failing = queued.filter((pr) => checksFailing(pr).length > 0);
  // Open work the queue is not managing. Arming is what queues a pull request
  // (ADR-0045), so an unarmed one is not late in the queue -- it is not in it.
  // Reporting only the armed ones made those two states indistinguishable: on
  // the day this was written three of five open pull requests were invisible
  // here, and no change to the ordering could ever have reached them.
  const unqueued = prs.filter((pr) => !isQueued(pr));
  // Armed, current, and carrying no check runs at all. The queue cannot start a
  // workflow, but it can stop this reading as an ordinary wait.
  const silent = queued.filter((pr) => isUpToDate(pr) && checksAbsent(pr));
  const context = { queued, blocked, failing, unqueued, silent };

  if (queued.length === 0) {
    return { kind: "idle", reason: "nothing armed", ...context };
  }

  // Immediately after a merge, GitHub recomputes mergeability and every entry
  // reads UNKNOWN for a few seconds. That is a transient state, not a quiet
  // queue, and treating it as quiet costs a whole interval per merge -- with a
  // full interval between ticks that was up to a minute of dead time on every
  // single delivery. Naming it lets the caller come back quickly instead.
  if (queued.every((pr) => pr.mergeStateStatus === "UNKNOWN")) {
    return {
      kind: "settling",
      reason: "GitHub is still recomputing mergeability",
      ...context,
    };
  }

  // A branch that is up to date and still resolving is the one that is about to
  // land. Updating anything now would only make it stale behind that merge.
  const landing = queued.find(
    (pr) =>
      isUpToDate(pr) && verdictPending(pr) && checksFailing(pr).length === 0,
  );
  if (landing) {
    const since = Date.parse(landing.updatedAt ?? landing.createdAt ?? 0) || 0;
    if (now - since < stalledAfterMs) {
      return {
        kind: "wait",
        pr: landing,
        reason: `#${landing.number} is up to date and about to land`,
        ...context,
      };
    }
    // Fall through: the in-flight run is older than the stall threshold, so it
    // is treated as never going to report rather than as a live build.
  }

  // A branch that cannot be updated automatically needs a human or its own
  // agent; it must not hold up everything behind it. A branch whose checks are
  // still running is skipped too — updating it again would only restart the run
  // we just decided to stop waiting for.
  // A branch still behind the base is updated whatever its rollup says -- the
  // update is itself what triggers the run it is missing. Only the up-to-date
  // arm excludes a silent head, because there the queue has nothing left to do
  // that could produce one, and falling through to `wait` is the wedge.
  const next = queued.find(
    (pr) =>
      !checksRunning(pr) &&
      (pr.mergeStateStatus === "BEHIND" ||
        (isUpToDate(pr) &&
          !checksAbsent(pr) &&
          checksFailing(pr).length === 0)),
  );
  if (!next) {
    return {
      kind: "idle",
      reason: "no armed branch is behind and idle",
      ...context,
    };
  }
  if (isUpToDate(next)) {
    return {
      kind: "wait",
      pr: next,
      reason: `#${next.number} is up to date and waiting on GitHub to merge it`,
      ...context,
    };
  }
  return { kind: "update", pr: next, ...context };
}

/** Render one tick's decision for a human reading the log. */
export function describe(action) {
  const lines = [`queue: ${action.queued.length} armed`];
  for (const pr of action.queued) {
    const marks = [];
    if (action.blocked.includes(pr)) marks.push("CONFLICT");
    if (action.failing.includes(pr)) marks.push("FAILING");
    if (checksRunning(pr)) marks.push("running");
    lines.push(
      `  #${String(pr.number).padEnd(4)} ${String(pr.mergeStateStatus).padEnd(9)} ` +
        `${marks.join(" ").padEnd(18)} ${(pr.title ?? "").slice(0, 44)}`,
    );
  }
  if (action.kind === "update")
    lines.push(`-> updating #${action.pr.number} from main`);
  if (action.kind === "wait") lines.push(`-> waiting: ${action.reason}`);
  if (action.kind === "idle") lines.push(`-> idle: ${action.reason}`);
  if (action.kind === "settling")
    lines.push(`-> settling: ${action.reason}, looking again shortly`);
  for (const pr of action.blocked) {
    lines.push(
      `   #${pr.number} has a real conflict; it needs its own worktree, not the queue`,
    );
  }
  // "Waiting on GitHub to merge it" and "no workflow ever ran" were the same
  // line, which is what made a silent head able to hold the queue unnoticed.
  for (const pr of action.silent ?? []) {
    lines.push(
      `   #${pr.number} is armed and up to date but no check has reported; nothing will merge it until one does`,
    );
  }
  // An unarmed pull request is not last in the queue, it is absent from it, and
  // silence here reads exactly like an empty backlog. Naming them is reporting,
  // not policy: arming is still what queues a pull request (ADR-0045).
  for (const pr of action.unqueued ?? []) {
    lines.push(
      `   #${String(pr.number).padEnd(4)} ${String(pr.mergeStateStatus).padEnd(9)} ` +
        `not queued: nobody armed it`,
    );
  }
  return lines.join("\n");
}

const gh = (args) =>
  execFileSync("gh", args, {
    encoding: "utf8",
    maxBuffer: 1 << 26,
    stdio: ["pipe", "pipe", "pipe"],
  });

export function readQueue() {
  return JSON.parse(
    gh([
      "pr",
      "list",
      "--state",
      "open",
      "--limit",
      "100",
      "--json",
      `${PR_FIELDS},updatedAt`,
    ]),
  );
}

/**
 * The real `verifyDirty`: fetch the two refs the answer depends on, then ask
 * `git merge-tree` whether they actually conflict. It never touches the
 * working tree, the index, or any ref -- `--write-tree` writes a tree object
 * to the object database and nothing else, so this is safe to call mid-tick
 * against whatever the caller happens to have checked out.
 *
 * `git merge-tree` exits non-zero when the merge has conflicts and zero when
 * it does not (`execFileSync` throwing is exactly that non-zero exit), so a
 * clean merge and a real conflict are already distinguished by the exit code
 * before any output is inspected. On a fetch failure or any other unexpected
 * error, this reports the conflict as real: a cached `DIRTY` that turns out to
 * be correct costs one extra hand-back, but treating an unverifiable branch as
 * automatically clean could update a branch that genuinely does not merge.
 */
export function verifyDirtyWithGit(pr) {
  try {
    execFileSync(
      "git",
      ["fetch", "origin", "main", pr.headRefName, "--quiet"],
      {
        stdio: ["pipe", "pipe", "pipe"],
      },
    );
    execFileSync(
      "git",
      [
        "merge-tree",
        "--write-tree",
        "--name-only",
        "origin/main",
        `origin/${pr.headRefName}`,
      ],
      { encoding: "utf8", stdio: ["pipe", "pipe", "pipe"] },
    );
    return false;
  } catch {
    return true;
  }
}

/**
 * Whether an already-succeeded `update-branch` call actually produced the
 * merge it was expected to, rather than the tree its clean exit code merely
 * implied. `null` on either side means "could not verify" (a fetch failed, or
 * the pre-check itself hit a real conflict) and must never read as a
 * mismatch -- only two actually-computed, differing tree hashes do.
 *
 * gaps.d/update-branch-can-silently-drop-a-conflicts-losing-side.md: PR #507's
 * `update-branch` exited 0 and reported no conflict, yet the resulting tree
 * silently dropped one side of a real three-way merge -- no conflict marker,
 * no failing check, nothing in the normal signal path caught it.
 */
export function updateBranchMismatch(expectedTree, actualTree) {
  if (expectedTree === null || actualTree === null) return false;
  return expectedTree !== actualTree;
}

/**
 * The tree `git merge-tree --write-tree` would produce for `base` merged with
 * `head`, or `null` when the merge conflicts or either ref cannot be read --
 * both mean there is nothing to compare, not that the merge is fine.
 */
function mergeTreeHash(base, head) {
  try {
    const output = execFileSync(
      "git",
      ["merge-tree", "--write-tree", base, head],
      { encoding: "utf8", stdio: ["pipe", "pipe", "pipe"] },
    );
    return output.trim().split("\n")[0] || null;
  } catch {
    return null;
  }
}

/** The tree a ref currently points at, or `null` when the ref cannot be read. */
function refTreeHash(ref) {
  try {
    return execFileSync("git", ["rev-parse", `${ref}^{tree}`], {
      encoding: "utf8",
      stdio: ["pipe", "pipe", "pipe"],
    }).trim();
  } catch {
    return null;
  }
}

/**
 * The tree `origin/main` merged with `origin/<branch>` is expected to
 * produce right now, fetching first so a stale local remote-tracking ref
 * cannot silently compare against an old `main` or an old branch tip.
 */
export function expectedMergeTree(branch) {
  try {
    execFileSync("git", ["fetch", "origin", "main", branch, "--quiet"], {
      stdio: ["pipe", "pipe", "pipe"],
    });
  } catch {
    return null;
  }
  return mergeTreeHash("origin/main", `origin/${branch}`);
}

/**
 * The tree `origin/<branch>` actually points at after its update, re-fetching
 * first since `update-branch` moved the branch server-side, not locally.
 */
export function actualMergeTree(branch) {
  try {
    execFileSync("git", ["fetch", "origin", branch, "--quiet"], {
      stdio: ["pipe", "pipe", "pipe"],
    });
  } catch {
    return null;
  }
  return refTreeHash(`origin/${branch}`);
}

// --- is anybody watching? ---------------------------------------------------
//
// "Armed means finished" (ADR-0045, ADR-0062) rests on something taking the
// turns, and nothing verified that anything was. `make queue-watch` exists and
// is documented, but a watcher nobody started looks exactly like a watcher with
// nothing to do: silence. Measured over two days on this board, armed pull
// requests repeatedly sat BEHIND for hours and were advanced only when somebody
// happened to run the one-shot by hand.
//
// The whole mechanism is one timestamp: watch mode writes it every tick, and a
// one-shot run reads it. No process enumeration -- that would be per-platform,
// and AGENTS.md's toolchain rule is that everything runs identically on Linux,
// macOS, and Windows.

/** A heartbeat older than this is not a running watcher. */
export const WATCHER_STALE_MS = 5 * 60 * 1000;

const HEARTBEAT_FILE = "delivery-queue-watcher.json";

/**
 * Where the heartbeat lives: the common Git directory, not `target/`.
 *
 * It has to be somewhere both sides can see. The watcher runs in one checkout
 * and the one-shot is run from whichever worktree an agent happens to be in --
 * and `target/` is per-worktree, so a heartbeat written there would be invisible
 * to every other worktree and the note would fire constantly against a watcher
 * that is running fine. The common Git directory is the one location every
 * worktree of a clone resolves through, which is exactly why the artefact sweep
 * keeps its own fleet-wide state and lock there. It is also outside the working
 * tree, so nothing needs gitignoring.
 *
 * `null` when this is not a repository at all, which means say nothing rather
 * than guess.
 */
function commonGitDir() {
  try {
    return execFileSync("git", ["rev-parse", "--git-common-dir"], {
      encoding: "utf8",
      stdio: ["pipe", "pipe", "pipe"],
    }).trim();
  } catch {
    return null;
  }
}

/** The last beat's Unix milliseconds, or `null` for absent or unreadable. */
export function readWatcherHeartbeat(commonDir) {
  try {
    const beat = JSON.parse(
      readFileSync(join(commonDir, HEARTBEAT_FILE), "utf8"),
    );
    return typeof beat.at === "number" && !Number.isNaN(beat.at)
      ? beat.at
      : null;
  } catch {
    // Absent or corrupt reads as "no watcher", which is the safe direction: a
    // note nobody needed costs one line, a missing one costs a stalled queue.
    return null;
  }
}

export function writeWatcherHeartbeat(commonDir, now) {
  try {
    writeFileSync(
      join(commonDir, HEARTBEAT_FILE),
      `${JSON.stringify({ at: now }, null, 2)}\n`,
      "utf8",
    );
  } catch {
    // A heartbeat that cannot be written is a lost diagnostic, not a lost
    // queue. The watcher keeps taking turns either way.
  }
}

/**
 * Whether a heartbeat proves a watcher is running now. Pure.
 *
 * A beat from the future counts: a clock that reads ahead is a clock problem,
 * not evidence that nothing is running.
 */
export function watcherIsRunning(at, now, staleMs = WATCHER_STALE_MS) {
  if (typeof at !== "number" || Number.isNaN(at)) return false;
  return now - at < staleMs;
}

/**
 * What to say when armed work is waiting and nothing is advancing it, or `null`
 * for nothing worth saying. Pure.
 *
 * Silent when a watcher is beating, and silent when nothing is armed -- an
 * empty queue needs no watcher, and a note that appears when it is not
 * actionable is a note that stops being read. It distinguishes a watcher that
 * stopped from one that was never started, because those need different things
 * done about them.
 */
export function unattendedQueueNote(
  action,
  heartbeatAt,
  now,
  staleMs = WATCHER_STALE_MS,
) {
  const armed = action.queued?.length ?? 0;
  if (armed === 0) return null;
  if (watcherIsRunning(heartbeatAt, now, staleMs)) return null;
  const since =
    typeof heartbeatAt === "number" && !Number.isNaN(heartbeatAt)
      ? `last beat ${Math.max(0, Math.round((now - heartbeatAt) / 60_000))}m ago`
      : "none has ever beaten here";
  return (
    `no delivery-queue watcher is running (${since}), and ` +
    `${armed} armed pull request${armed === 1 ? " is" : "s are"} waiting for ` +
    `turns nobody is taking. Start one with \`make queue-watch\`.`
  );
}

const USAGE = `delivery-queue -- take branch-update turns in order (ADR-0062)

  node scripts/delivery-queue.mjs [--watch] [--dry-run]

  (no flags)  one tick: show the queue, update whichever branch's turn it is
  --watch     keep ticking every 60s until stopped
  --dry-run   decide and explain, change nothing
  --no-sweep  skip build-artefact hygiene entirely

It also carries build-artefact hygiene, because that work has no home of its
own: the agent that filled a cache has finished by the time it is safe to
delete, so a CLI nobody remembers to run reclaims nothing. This process is
already persistent and already single-owner, so the sweep rides on it -- once at
startup, then every few hours, holding a lock in the common Git directory so two
watchers cannot sweep at once. It reports by default and only deletes when this
queue is applying rather than in --dry-run.

A pull request with auto-merge armed is a queued one (ADR-0045), ordered by
when it was armed. Exactly one branch is brought up to date at a time, because
'main' requires branches to be current and every merge makes the others stale --
refreshing them all at once burns a check run per branch per merge and never
drains.

It never merges. Merging stays with GitHub's auto-merge behind the same required
checks, so this cannot become a second way into 'main'.

It reports and steps over, rather than waiting on:
  a real conflict   reconcile it in its own worktree (ADR-0038)
  failing checks    updating would only burn CI to fail again
  a wedged check    stops being waited on after 45 minutes

Nothing depends on it running: an unattended queue just means branches go stale
the way they did before. A one-shot run says so when it notices, because
"unattended" and "nothing to do" otherwise look identical from outside.`;

function main() {
  if (process.argv.includes("--help") || process.argv.includes("-h")) {
    console.log(USAGE);
    return;
  }
  const apply = !process.argv.includes("--dry-run");
  const watch = process.argv.includes("--watch");
  const sweeping = !process.argv.includes("--no-sweep");

  // Hygiene is deliberately outside `tick`: a disk walk must never sit on the
  // path that decides whose branch is updated next, and a sweep that throws
  // must not stop the queue draining. Its own cadence and lock make calling it
  // every tick cheap -- all but one call in several hours returns "not due",
  // which it says by printing nothing.
  let saidLast = null;
  const sweepNow = () => {
    if (!sweeping) return;
    try {
      const said = sweepAnnouncement(
        execFileSync(process.execPath, sweepArgs(apply), {
          encoding: "utf8",
          cwd: process.cwd(),
        }),
        saidLast,
      );
      if (said !== null) {
        saidLast = said;
        console.log(said);
      }
    } catch (error) {
      // Housekeeping failing is not the queue failing.
      console.log(`artefact-sweep skipped: ${error.message.split("\n")[0]}`);
    }
  };

  const tick = () => {
    const action = nextAction(readQueue(), Date.now(), {
      verifyDirty: verifyDirtyWithGit,
    });
    console.log(describe(action));
    if (action.kind !== "update") return action;
    if (!apply) {
      console.log("(dry run: no branch was updated)");
      return action;
    }
    const branch = action.pr.headRefName;
    const expectedTree = expectedMergeTree(branch);
    try {
      gh(["pr", "update-branch", String(action.pr.number)]);
      const actualTree = actualMergeTree(branch);
      if (updateBranchMismatch(expectedTree, actualTree)) {
        // The exact defect gaps.d/update-branch-can-silently-drop-a-conflicts-
        // losing-side.md records: a clean exit and no reported conflict, yet
        // the resulting tree is not the merge it was supposed to be.
        console.log(
          `updated #${action.pr.number} BUT ITS TREE DOES NOT MATCH THE EXPECTED MERGE -- ` +
            `gh pr update-branch may have silently dropped a side. Reconcile ` +
            `#${action.pr.number} by hand: fetch and diff origin/${branch} ` +
            `against a fresh local \`git merge origin/main\` before trusting it.`,
        );
      } else {
        console.log(`updated #${action.pr.number}`);
      }
    } catch (error) {
      // `update-branch` fails when GitHub cannot merge main in cleanly. That is
      // information, not a crash: the branch needs reconciling in its own
      // worktree (ADR-0038), and the queue should carry on without it.
      const detail = (error.stdout || error.stderr || error.message)
        .trim()
        .split("\n")
        .pop();
      console.log(`could not update #${action.pr.number}: ${detail}`);
    }
    return action;
  };

  const commonDir = commonGitDir();

  if (!watch) {
    sweepNow();
    const action = tick();
    // Read after the tick, never before: this run has just done by hand exactly
    // what a watcher would have done, so the question the note answers is "is
    // anything going to do that again without me?".
    const note =
      commonDir &&
      unattendedQueueNote(action, readWatcherHeartbeat(commonDir), Date.now());
    if (note) console.log(`\n${note}`);
    return;
  }
  // Watch mode is the agent: one tick, wait, tick again. The interval is longer
  // than a check run takes to appear so a tick cannot mistake "not started yet"
  // for "nothing in flight" and update a second branch.
  //
  // The exception is `settling`: GitHub is mid-recompute and every entry reads
  // UNKNOWN, which resolves in seconds. Waiting a full interval there wasted up
  // to a minute on every merge, so that case alone comes back quickly. It is
  // safe to shorten because a settling tick has, by construction, done nothing.
  const intervalMs = 60_000;
  const settlingMs = 5_000;
  console.log(
    `delivery queue watching every ${intervalMs / 1000}s -- Ctrl-C to stop\n`,
  );
  let timer;
  const beat = () => {
    if (commonDir) writeWatcherHeartbeat(commonDir, Date.now());
  };
  const schedule = (delay) => {
    timer = setTimeout(() => {
      console.log(`\n--- ${new Date().toISOString()} ---`);
      beat();
      let kind = "idle";
      try {
        kind = tick().kind;
      } catch (error) {
        // A transient API failure must not kill the agent; the next tick retries.
        console.log(
          `tick failed, retrying next interval: ${error.message.split("\n")[0]}`,
        );
      }
      sweepNow();
      schedule(kind === "settling" ? settlingMs : intervalMs);
    }, delay);
  };
  beat();
  tick();
  schedule(intervalMs);
  process.on("SIGINT", () => {
    clearTimeout(timer);
    console.log("\ndelivery queue stopped");
    process.exit(0);
  });
}

if (
  process.argv[1] &&
  import.meta.url.endsWith(process.argv[1].replace(/\\/g, "/"))
) {
  main();
}
