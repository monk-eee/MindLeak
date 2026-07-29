// Auto-merge guard (ADR-0045 clause 2: one arbiter per shared mutable resource).
//
// A pull request's merge decision has two writers: the agent pushing commits,
// and whoever arms auto-merge. Arming is a promise to merge whatever is on the
// branch the moment checks go green — a promise made about work someone else
// has not finished. PR #37 merged at 08:09:21Z; the next commit landed 13
// seconds later and four commits never reached main, with nothing reported.
//
// So arming means finished. This module answers one question — is this branch
// promised away? — and is kept separate from the publisher so the decision can
// be tested without a network, a token, or a real `gh`.

import { execFileSync } from "node:child_process";

/**
 * The number of an open pull request with auto-merge armed, or `null`.
 *
 * `null` covers three genuinely different situations — no pull request, one
 * that is closed, and one nobody armed — because the caller does the same thing
 * in all three: nothing.
 */
export const armedPullRequestNumber = (raw) => {
  if (!raw) return null;
  let pullRequest;
  try {
    pullRequest = JSON.parse(raw);
  } catch {
    // Unparseable means unknown, and a guard must not invent an answer it does
    // not have. Blocking on its own blindness would make it unsatisfiable.
    return null;
  }
  if (pullRequest?.state !== "OPEN") return null;
  if (!pullRequest.autoMergeRequest) return null;
  return typeof pullRequest.number === "number" ? pullRequest.number : null;
};

/** Raw `gh` output for a branch's pull request, or `null` when unavailable. */
export const queryPullRequest = (branch, cwd) => {
  const gh = process.env.MINDLEAK_GH_BIN || "gh";
  try {
    return execFileSync(
      gh,
      ["pr", "view", branch, "--json", "number,state,autoMergeRequest"],
      { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
    );
  } catch {
    return null;
  }
};

/** What to tell someone whose branch is already promised away. */
export const armedRefusal = (number, branch) =>
  `pull request #${number} has auto-merge armed on ${branch}; it can merge without this commit.\n` +
  `  Disarm it first:  gh pr merge ${number} --disable-auto\n` +
  "  Then publish, and re-arm when the branch is actually finished.";

const gh = (args, cwd) => {
  const bin = process.env.MINDLEAK_GH_BIN || "gh";
  execFileSync(bin, args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
};

/** Withdraw the merge promise so the branch can be written to. */
export const disarmPullRequest = (number, cwd) =>
  gh(["pr", "merge", String(number), "--disable-auto"], cwd);

/** Re-make the promise, now about the tip that was actually published. */
export const rearmPullRequest = (number, cwd) =>
  gh(["pr", "merge", String(number), "--merge", "--auto"], cwd);

/**
 * Publish to a branch whose merge is already promised away.
 *
 * Refusing the push was the first answer, and it holds the invariant — one
 * writer to the merge decision — at the cost of making every follow-up commit a
 * manual disarm/re-arm dance, which is precisely the kind of ceremony people
 * skip at 6pm. Worse, the escape hatch it pushes you toward is arming late,
 * which means somebody has to sit and watch the pull request instead.
 *
 * Withdrawing the promise, writing, and re-making it holds the same invariant
 * more strictly: at no point is there an armed promise about a branch that is
 * being written to, and the promise that ends up armed describes the tip that
 * was actually published rather than whatever happened to be there when
 * somebody clicked. Nobody merges by hand and nobody disarms by hand.
 *
 * Every step is injected so the ordering can be proven without a network, a
 * token, or a real `gh` — the ordering *is* the safety property, and it is the
 * one thing a live smoke test would be least able to observe.
 *
 * Re-arming is attempted even when the push fails, because a failed push leaves
 * the branch exactly as the promise already described it. If re-arming itself
 * fails the pull request is left disarmed, which is the safe direction: work
 * sits unmerged and visible rather than merging something nobody promised.
 */
export const publishPromisedBranch = ({ number, disarm, push, rearm }) => {
  if (number === null) {
    push();
    return { cycled: false, pushed: true, rearmed: null, rearmError: null };
  }

  disarm(number);

  let pushed = false;
  let pushError = null;
  try {
    push();
    pushed = true;
  } catch (error) {
    pushError = error;
  }

  let rearmed = false;
  let rearmError = null;
  try {
    rearm(number);
    rearmed = true;
  } catch (error) {
    rearmError = error;
  }

  if (pushError) throw pushError;
  return { cycled: true, pushed, rearmed, rearmError };
};

/** What to tell someone whose pull request is disarmed and could not be re-armed. */
export const rearmFailure = (number) =>
  `auto-merge could not be re-armed on pull request #${number}; it is disarmed and will not merge.\n` +
  `  Re-arm it:  gh pr merge ${number} --merge --auto`;
