// Worktree ownership (ADR-0038). Worktree isolation only holds if each linked
// worktree has exactly one writer. Git isolates files, the index, and branch
// selection — but not *who* is allowed to type in a given checkout. An agent
// that walks into a peer's worktree and commits there races that peer's edits,
// and pre-commit's stash/restore cycle can land on top of work they are still
// writing; the resulting failure surfaces in their branch, naming files the
// intruder never touched, which is what makes it so expensive to unpick.
//
// So a linked worktree records the session that first commits in it, and every
// later commit must come from the same session. The marker lives in the
// per-worktree git dir, so it is never part of any commit, never collides
// between worktrees, and disappears with the worktree.
//
// Platform-agnostic: git + node only.

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

export const MARKER_NAME = "lodestar-owner";

/// Decide what to do given the recorded owner and the acting session. Pure, so
/// the interesting cases are testable without a git repository.
export function ownershipVerdict({
  recorded,
  session,
  adopt = false,
  linked = true,
}) {
  // A human running git directly is not a fleet agent and must not be locked
  // out of their own checkout.
  if (!session) return { action: "skip", reason: "no session identity" };
  // The primary checkout is shared by construction; ownership is meaningless
  // there, and scoped-commit already guards its distinct failure mode.
  if (!linked) return { action: "skip", reason: "primary checkout" };
  if (adopt) return { action: "record", reason: "deliberate handover" };
  if (!recorded)
    return { action: "record", reason: "first commit claims the worktree" };
  if (recorded === session) return { action: "allow", reason: "owner" };
  return {
    action: "refuse",
    reason: "owned by another agent",
    owner: recorded,
  };
}

export function refusalMessage({ owner, session }) {
  return (
    "this worktree belongs to another agent.\n" +
    `  worktree owner : ${owner}\n` +
    `  you            : ${session}\n` +
    "\nA linked worktree has one writer. Committing here races that agent's edits, and\n" +
    "the pre-commit stash can restore over work they are still writing — the failure\n" +
    "then surfaces in their branch, naming files you never touched.\n" +
    "\nWork in your own worktree (ADR-0038):\n" +
    "  git worktree add ../MindLeak-<workstream> -b fleet/<workstream> origin/main\n" +
    "\nIf this worktree was genuinely handed over to you, take it deliberately with " +
    "--adopt-worktree."
  );
}

const capture = (args, cwd) =>
  execFileSync("git", args, { cwd, encoding: "utf8" }).trim();

const ghCapture = (args, cwd) =>
  execFileSync("gh", args, { cwd, encoding: "utf8" }).trim();

// Adopting a lapsed-lease worktree only ever consults the task ledger and
// local git state, and both can be genuinely clean while the original owner
// already pushed and opened a PR seconds before their lease lapsed --
// observed for real (gaps.d/rescuing-a-lapsed-lease-can-duplicate-a-published-pr.md):
// a rescue cherry-picked an already-shipped commit onto a fresh branch,
// republishing it as a second, duplicate PR. Neither the ledger nor local git
// reads GitHub, so this is the one place that does. Never a hard refusal --
// a closed/abandoned PR, or a stale/unauthenticated `gh` call, must not block
// a genuine rescue; the point is to make the possibility loud, not to gate it.
export function checkExistingPullRequests({
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
    return { checked: false, pullRequests: [] };
  }
  try {
    return { checked: true, pullRequests: JSON.parse(raw) };
  } catch {
    return { checked: false, pullRequests: [] };
  }
}

/// Render the pre-flight result as a message for the rescuer, or `null` when
/// there is nothing to say. Pure, so every case is testable without `gh`.
export function existingPullRequestWarning({ branch, checked, pullRequests }) {
  if (!checked) {
    return (
      `worktree-owner: could not check GitHub for an existing pull request on '${branch}'\n` +
      "  (gh unavailable, unauthenticated, or the call failed) -- adopting anyway.\n" +
      `  Verify by hand before publishing: gh pr list --head ${branch} --state all`
    );
  }
  if (!pullRequests || pullRequests.length === 0) return null;
  const named = pullRequests
    .map((pr) => `#${pr.number} (${pr.state}) ${pr.url}`)
    .join(", ");
  return (
    `worktree-owner: '${branch}' already has a published pull request: ${named}\n` +
    "  Rescuing this worktree can republish work its original owner already shipped\n" +
    "  (gaps.d/rescuing-a-lapsed-lease-can-duplicate-a-published-pr.md). Check the PR\n" +
    "  before cherry-picking or committing further."
  );
}

/// Resolve the verdict for a working tree, recording ownership when it is
/// unclaimed. Returns the verdict so callers decide how loudly to fail.
export function checkWorktreeOwnership({
  cwd = process.cwd(),
  adopt = false,
} = {}) {
  const session = (process.env.LODESTAR_SESSION_ID ?? "").trim();
  if (!session) return ownershipVerdict({ recorded: "", session, adopt });

  const gitDir = capture(["rev-parse", "--absolute-git-dir"], cwd);
  const commonDir = capture(
    ["rev-parse", "--path-format=absolute", "--git-common-dir"],
    cwd,
  ).replace(/\/$/, "");
  const linked = gitDir.toLowerCase() !== commonDir.toLowerCase();

  const marker = join(gitDir, MARKER_NAME);
  const recorded = existsSync(marker)
    ? readFileSync(marker, "utf8").trim()
    : "";
  const verdict = ownershipVerdict({ recorded, session, adopt, linked });

  if (verdict.action === "record") writeFileSync(marker, `${session}\n`);
  return { ...verdict, session };
}

// Hook entry point. `--stage=post-checkout` runs right after `git worktree
// add` completes: git ignores this hook's exit code for the checkout it just
// performed, so there is nothing to block. Its only job is to write the
// marker on a still-unclaimed worktree via the exact verdict logic above, and
// warn rather than refuse if it somehow finds one already owned by a peer.
// The default (commit) stage keeps refusing outright, since that one still
// guards a real write.
if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  const isPostCheckout = process.argv.includes("--stage=post-checkout");
  const adopt = process.argv.includes("--adopt-worktree");
  const verdict = checkWorktreeOwnership({ adopt });
  if (adopt) {
    const branch = capture(["branch", "--show-current"], process.cwd());
    const warning = existingPullRequestWarning({
      branch,
      ...checkExistingPullRequests({ branch }),
    });
    if (warning) console.error(warning);
  }
  if (verdict.action === "refuse") {
    if (isPostCheckout) {
      console.error(
        `worktree-owner: this worktree already belongs to another agent — ${refusalMessage(verdict)}`,
      );
    } else {
      console.error(
        `worktree-owner: refusing to commit — ${refusalMessage(verdict)}`,
      );
      process.exit(4);
    }
  }
}
