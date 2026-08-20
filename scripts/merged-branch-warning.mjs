// gaps.d/a-branch-whose-pr-merged-can-still-take-commits.md: a branch is not
// terminal the way a task or a PR is, so nothing previously refused or even
// noticed a commit landing on one whose pull request had already merged --
// observed twice in one session, both times recovered only by chance (a
// completed task's provenance trail, and a `git log` glance before
// committing). Neither the task ledger nor local git state can see GitHub, so
// this is the one place that does. `abandon` already refuses this same shape
// of mistake for retiring a task; this is its publish-time counterpart.
//
// A standalone module rather than living inside canonical-push.mjs: that
// script runs its whole publish flow as top-level side effects the moment it
// is loaded (there is no `import.meta.url === entry point` guard), so
// importing a function from it for a unit test would run a real push. These
// two functions have no such side effect and are safe to import directly.

import { execFileSync } from "node:child_process";

const ghCapture = (args, cwd) =>
  execFileSync("gh", args, { cwd, encoding: "utf8" }).trim();

/**
 * The current branch's most recent pull request, or `null` when it has never
 * had one. Ordered by PR number, not API response order: GitHub does not
 * guarantee `pr list`'s ordering, but PR numbers are assigned once and never
 * reused, so the highest one is unambiguously the most recent regardless of
 * how the results came back.
 */
export function mostRecentPullRequest({
  branch,
  cwd = process.cwd(),
  capture: ghCall = ghCapture,
} = {}) {
  let raw;
  try {
    raw = ghCall(
      [
        "pr",
        "list",
        "--head",
        branch,
        "--state",
        "all",
        "--json",
        "number,state,url",
      ],
      cwd,
    );
  } catch {
    return { checked: false, pullRequest: null };
  }
  let pullRequests;
  try {
    pullRequests = JSON.parse(raw);
  } catch {
    return { checked: false, pullRequest: null };
  }
  if (!Array.isArray(pullRequests) || pullRequests.length === 0) {
    return { checked: true, pullRequest: null };
  }
  const mostRecent = pullRequests.reduce((latest, pr) =>
    !latest || pr.number > latest.number ? pr : latest,
  );
  return { checked: true, pullRequest: mostRecent };
}

/// Never a refusal: pushing more commits onto a branch whose PR already
/// merged is not itself harmful, and opening a fresh PR from the same branch
/// afterward is the documented, working way to publish them. The harm this
/// guards is nobody remembering to take that step.
export function mergedBranchWarning({ branch, checked, pullRequest }) {
  if (!checked) {
    return (
      `could not check GitHub for '${branch}'s most recent pull request\n` +
      "  (gh unavailable, unauthenticated, or the call failed) -- pushing anyway."
    );
  }
  if (!pullRequest || pullRequest.state !== "MERGED") return null;
  return (
    `'${branch}' already has a MERGED pull request (#${pullRequest.number}, ${pullRequest.url}).\n` +
    `  These new commits will not appear in it -- open a fresh PR for them: gh pr create --head ${branch}`
  );
}
