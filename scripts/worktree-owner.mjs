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

// Hook entry point: refuse the commit outright.
if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  const verdict = checkWorktreeOwnership({
    adopt: process.argv.includes("--adopt-worktree"),
  });
  if (verdict.action === "refuse") {
    console.error(
      `worktree-owner: refusing to commit — ${refusalMessage(verdict)}`,
    );
    process.exit(4);
  }
}
