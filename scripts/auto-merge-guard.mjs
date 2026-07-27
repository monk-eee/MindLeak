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
