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

import {
  callTools,
  liveClaimHeldByAnother,
  resolveServer,
} from "./claim-gate.mjs";

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

// Deliberately not the trimming `capture` above: `.trim()` on a multi-line
// `git status --porcelain` result strips only the very start and end of the
// whole string, which eats the first line's leading status-code space and
// shifts every subsequent slice on that line by one column. Reproduced for
// real: " M tracked.txt\n?? x.sql" trims to "M tracked.txt\n?? x.sql", and
// slicing the (now one-column-short) first line at a fixed offset silently
// drops its leading character.
const captureUntrimmed = (args, cwd) =>
  execFileSync("git", args, { cwd, encoding: "utf8" });

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

// The adopt path's only pre-flight was a *remote* lookup (the PR check
// above), and that leaves local state completely unconsulted -- an
// ownership-transfer command whose only safety check is a remote lookup will
// happily take *local* work, because the two are independent
// (gaps.d/adopt-worktree-takes-a-peers-uncommitted-work.md). Observed for
// real: a worktree left clean and idle after a claim refusal was adopted by
// a peer who began writing there; roughly twenty minutes later its original
// creator ran --adopt-worktree believing it was still their own idle
// worktree, and the peer's five modified files plus one untracked migration
// were about to transfer ownership silently -- caught only because `git
// switch` in the same command happened to refuse for an unrelated reason.
// `git status --porcelain` sees exactly that: staged, unstaged, and
// untracked changes alike, with no dependency on what branch or commit is
// checked out.
export function checkWorkingTreeDirty({
  cwd = process.cwd(),
  capture: gitCall = captureUntrimmed,
} = {}) {
  let raw;
  try {
    raw = gitCall(["status", "--porcelain"], cwd);
  } catch {
    return { checked: false, paths: [] };
  }
  const paths = raw
    .split("\n")
    .map((line) => line.replace(/\r$/, ""))
    .filter((line) => line.length > 0)
    // `git status --porcelain` prefixes each line with a two-character status
    // code and a space; trimming the whole line first would eat a leading
    // space status (e.g. " M path") and shift this slice, so only the
    // extracted path itself is trimmed.
    .map((line) => line.slice(3).trim());
  return { checked: true, paths };
}

/// Render the dirty-tree pre-flight as a message for the rescuer, or `null`
/// when there is nothing to say. Pure, so every case is testable without git.
///
/// Deliberately advisory, exactly like `existingPullRequestWarning`: a
/// worktree can be genuinely, legitimately dirty at handover (mid-fix,
/// waiting on a human decision), and refusing outright would break the
/// rescue case gaps.d/rescuing-a-lapsed-lease-can-duplicate-a-published-pr.md
/// exists to support. The point is to make an invisible transfer loud, not
/// to gate it -- tightening this into a refusal is the ADR-sized decision
/// the gap fragment names and defers.
export function dirtyWorkingTreeWarning({ checked, paths }) {
  if (!checked) {
    return (
      "worktree-owner: could not check whether this worktree has uncommitted changes\n" +
      "  (git status failed) -- adopting anyway. Verify by hand: git status"
    );
  }
  if (!paths || paths.length === 0) return null;
  const named = paths.slice(0, 10).join(", ");
  const more = paths.length > 10 ? ` (+${paths.length - 10} more)` : "";
  return (
    `worktree-owner: this worktree has uncommitted changes: ${named}${more}\n` +
    "  Adopting it can take over a peer's live, unfinished work rather than a genuinely\n" +
    "  idle worktree (gaps.d/adopt-worktree-takes-a-peers-uncommitted-work.md). Confirm\n" +
    "  no one else is actively writing here before committing."
  );
}

// ADR-0130: unlike the dirty-tree and PR checks above, a live Lodestar claim
// on this exact branch, held by someone else, is not an ambiguous signal --
// `task_claim`'s own compare-and-swap already treats it as an unconditional
// loss. Reuses the exact resolveServer/callTools plumbing
// scripts/canonical-push.mjs already drives Lodestar through, rather than
// inventing a second way to reach the same server.
export function checkActiveClaimOnBranch({
  branch,
  session,
  cwd = process.cwd(),
  now = Date.now() / 1000,
  capture: gitCall = capture,
  resolveServer: resolve = resolveServer,
  callTools: call = callTools,
} = {}) {
  if (!session) return { checked: false, claim: null };
  let repoRoot;
  try {
    repoRoot = gitCall(["rev-parse", "--show-toplevel"], cwd);
  } catch {
    return { checked: false, claim: null };
  }
  const server = resolve(repoRoot, "lodestar");
  if (!server) return { checked: false, claim: null };
  let board;
  try {
    [board] = call(server, cwd, [
      {
        name: "task_query",
        arguments: {
          view: "board",
          include_terminal: false,
          branch,
          detail: false,
        },
      },
    ]);
  } catch {
    return { checked: false, claim: null };
  }
  const tasks = Array.isArray(board) ? board : (board?.tasks ?? []);
  return {
    checked: true,
    claim: liveClaimHeldByAnother(tasks, branch, session, now),
  };
}

/// Render the active-claim pre-flight as a message for the rescuer, or
/// `null` when there is nothing to say. Refusal, not a warning: this is the
/// one adopt-path signal ADR-0130 treats as unconditional rather than
/// advisory, because it mirrors what `task_claim` itself would already
/// refuse to hand over.
export function activeClaimRefusal({ branch, checked, claim }) {
  if (!checked) {
    return (
      `worktree-owner: could not check Lodestar for a live claim on '${branch}'\n` +
      "  (no server binary resolved, or the ledger is unreachable) -- adopting anyway.\n" +
      "  Verify by hand before committing: ask whoever might be working on this branch."
    );
  }
  if (!claim) return null;
  return (
    `worktree-owner: '${branch}' is claimed by another session and its lease has not\n` +
    `  expired (owner ${claim.owner}, task ${claim.id}). Adopting this worktree would take\n` +
    "  over exactly what task_claim itself would refuse to hand over (ADR-0130).\n" +
    "  If this is a genuine, deliberate handover, re-run with --override-active-claim."
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
  const overrideActiveClaim = process.argv.includes("--override-active-claim");
  const verdict = checkWorktreeOwnership({ adopt });
  if (adopt) {
    const branch = capture(["branch", "--show-current"], process.cwd());
    const prWarning = existingPullRequestWarning({
      branch,
      ...checkExistingPullRequests({ branch }),
    });
    if (prWarning) console.error(prWarning);
    const dirtyWarning = dirtyWorkingTreeWarning(checkWorkingTreeDirty());
    if (dirtyWarning) console.error(dirtyWarning);
    // ADR-0130: refusal, not a warning -- the one adopt-path signal treated
    // as unconditional, because it mirrors what task_claim itself would
    // already refuse to hand over. Checked last so the PR/dirty warnings
    // above still print even when this refuses.
    const activeClaim = checkActiveClaimOnBranch({
      branch,
      session: verdict.session,
    });
    const claimRefusal = activeClaimRefusal(activeClaim);
    if (claimRefusal) {
      if (activeClaim.claim && !overrideActiveClaim) {
        console.error(claimRefusal);
        process.exit(4);
      }
      console.error(
        activeClaim.claim
          ? `${claimRefusal}\n  Proceeding anyway: --override-active-claim was given.`
          : claimRefusal,
      );
    }
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
