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
export function nextAction(prs, now, { stalledAfterMs = 45 * 60 * 1000 } = {}) {
  const queued = queueOrder(prs);
  const blocked = queued.filter((pr) => pr.mergeStateStatus === "DIRTY");
  const failing = queued.filter((pr) => checksFailing(pr).length > 0);
  // Open work the queue is not managing. Arming is what queues a pull request
  // (ADR-0045), so an unarmed one is not late in the queue -- it is not in it.
  // Reporting only the armed ones made those two states indistinguishable: on
  // the day this was written three of five open pull requests were invisible
  // here, and no change to the ordering could ever have reached them.
  const unqueued = prs.filter((pr) => !isQueued(pr));
  const context = { queued, blocked, failing, unqueued };

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
      isUpToDate(pr) && checksRunning(pr) && checksFailing(pr).length === 0,
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
  const next = queued.find(
    (pr) =>
      !checksRunning(pr) &&
      (pr.mergeStateStatus === "BEHIND" ||
        (isUpToDate(pr) && checksFailing(pr).length === 0)),
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

const USAGE = `delivery-queue -- take branch-update turns in order (ADR-0062)

  node scripts/delivery-queue.mjs [--watch] [--dry-run]

  (no flags)  one tick: show the queue, update whichever branch's turn it is
  --watch     keep ticking every 60s until stopped
  --dry-run   decide and explain, change nothing

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
the way they did before.`;

function main() {
  if (process.argv.includes("--help") || process.argv.includes("-h")) {
    console.log(USAGE);
    return;
  }
  const apply = !process.argv.includes("--dry-run");
  const watch = process.argv.includes("--watch");
  const tick = () => {
    const action = nextAction(readQueue(), Date.now());
    console.log(describe(action));
    if (action.kind !== "update") return action.kind;
    if (!apply) {
      console.log("(dry run: no branch was updated)");
      return action.kind;
    }
    try {
      gh(["pr", "update-branch", String(action.pr.number)]);
      console.log(`updated #${action.pr.number}`);
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
    return action.kind;
  };

  if (!watch) {
    tick();
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
  const schedule = (delay) => {
    timer = setTimeout(() => {
      console.log(`\n--- ${new Date().toISOString()} ---`);
      let kind = "idle";
      try {
        kind = tick();
      } catch (error) {
        // A transient API failure must not kill the agent; the next tick retries.
        console.log(
          `tick failed, retrying next interval: ${error.message.split("\n")[0]}`,
        );
      }
      schedule(kind === "settling" ? settlingMs : intervalMs);
    }, delay);
  };
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
