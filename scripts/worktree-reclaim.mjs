// Reclaim the disk the fleet leaves behind: merged worktrees, their branches
// (local and remote), and build output.
//
// This exists because cleanup never happens on goodwill. The agent that created
// a worktree has finished and moved on by the time it is safe to remove, so the
// mess is always somebody else's, and it grows every time the fleet works
// correctly. Measured 2026-07-30: 88 worktrees, 86 carrying target/, 61 carrying
// node_modules, one sampled target/ holding 82,891 entries. That is what made
// the editor slow enough to be unusable.
//
// The failure mode of a cleanup tool is deleting work somebody still needed, and
// no report can be un-deleted. So reporting is the default, acting is explicit,
// and every refusal below comes from something that has actually gone wrong in
// this repository rather than from imagination.
//
// Platform-agnostic: node + git only. Usage:
//   node scripts/worktree-reclaim.mjs              report what could be reclaimed
//   node scripts/worktree-reclaim.mjs --reclaim    remove worktrees, local branches, build output
//   node scripts/worktree-reclaim.mjs --reclaim --remote   also delete merged remote branches
//   ... --artifacts-only   leave worktrees and branches, take only build output

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

import {
  callTools,
  resolveServer,
  unreadableBoardGuidance,
} from "./claim-gate.mjs";
import { removeTreeSafely } from "./fs-retry.mjs";
import { MARKER_NAME } from "./worktree-owner.mjs";

/** Branches that are never reclaimed whatever else is true of them. */
export const PROTECTED_BRANCHES = new Set(["main", "master"]);

/** Decide whether this copy of the safety-critical reclaim script is current. */
export function reclaimScriptFreshness({ fetched, matchesOrigin }) {
  if (!fetched) {
    return {
      current: false,
      reason: "could not fetch origin before checking this script's version",
    };
  }
  if (!matchesOrigin) {
    return {
      current: false,
      reason: "this script differs from origin/main",
    };
  }
  return { current: true, reason: null };
}

/**
 * Build output, named one directory at a time.
 *
 * Deliberately NOT the bare `target`. Cargo's output shares that parent with
 * things nothing can regenerate: `target/completion-offers` holds the exact
 * evidence bundle canonical-push computed for a task, and an aligned receipt
 * that is thrown away cannot be recovered — the claim window has moved on.
 * `target/tmp` holds the scratch a session is mid-way through using. Listing
 * `target` swept both, so the safety of this tool rested on the caller never
 * passing --artifacts-only to a tree it wanted to keep. Naming the
 * regenerable children instead makes the exclusion structural rather than a
 * habit somebody has to remember.
 *
 * Every kind here is reproducible from source by a command in the Makefile.
 */
export const ARTEFACT_KINDS = [
  { kind: "cargo-debug", rel: "target/debug" },
  { kind: "cargo-release", rel: "target/release" },
  { kind: "cargo-llvm-cov", rel: "target/llvm-cov-target" },
  { kind: "extension-node-modules", rel: "editors/vscode/node_modules" },
  { kind: "extension-out", rel: "editors/vscode/out" },
  { kind: "extension-coverage", rel: "editors/vscode/coverage" },
  { kind: "extension-vscode-test", rel: "editors/vscode/.vscode-test" },
];

/** Directories that hold build output and are never hand-edited. */
export const ARTIFACT_DIRECTORIES = ARTEFACT_KINDS.map((k) => k.rel);

/**
 * The kinds the fleet host is actively serving from, rather than merely
 * storing. Both are regenerable, which is why they are sweepable anywhere
 * else; on the host they are what the other agents are mid-call against.
 */
export const HOST_TOOLING_KINDS = new Set([
  "cargo-release",
  "extension-node-modules",
]);

/**
 * Whether one artefact directory may be swept, and if not, which rule stopped
 * it.
 *
 * Pure, and separate from `classifyWorktree` because the questions differ. That
 * one asks whether a whole worktree may be removed; this asks whether a
 * regenerable cache inside a tree that is *staying* may be deleted. A tree can
 * be perfectly alive and still be carrying tens of gigabytes nobody will read
 * again, which is where almost all of the 149.18 GiB measured on 2026-07-30
 * actually sat.
 *
 * Every fact arrives from the caller so each refusal is testable without
 * touching a disk. A cleanup tool tested only on what it deletes has not been
 * tested at all.
 */
export function classifyArtefact(candidate, options = {}) {
  const {
    now = Date.now(),
    minimumAgeMs = 0,
    graceMs = 0,
    session = null,
    openPrBranches = new Set(),
  } = options;
  const {
    kind,
    bare,
    primary,
    branch,
    dirty,
    landed,
    owner,
    building,
    modifiedAt,
  } = candidate;

  // The fleet host serves the tooling every agent is using right now: the MCP
  // binaries `resolveServer` resolves to, and the prettier/eslint the commit
  // hooks shell out to. Deleting either stops the fleet mid-flight, and the
  // symptom points nowhere near the cause -- "no lodestar-mcp binary found"
  // from a reclaim that inspected no worktree, or MODULE_NOT_FOUND from a hook
  // that modifies nothing.
  //
  // Keyed on `primary` rather than `bare` because a bare host is only one way
  // to arrange this. Measured 2026-08-13: this fleet serves from a NON-bare
  // checkout on `main`, which is clean, landed and idle, so every other
  // predicate passed and both directories were swept out from under it. A bare
  // host is always the primary worktree, so the case the original guard named
  // is still covered.
  //
  // One rule over a set of kinds rather than two special cases: what makes
  // these load-bearing is the checkout they sit in, not what they hold. The
  // host's target/debug stays ordinary stale output.
  if ((bare || primary) && HOST_TOOLING_KINDS.has(kind)) {
    return {
      sweep: false,
      reason: "the fleet host's build output serves the running tools",
    };
  }
  if (building) {
    return { sweep: false, reason: "a build is running here" };
  }
  if (dirty) {
    return { sweep: false, reason: "uncommitted or untracked changes" };
  }
  // A detached HEAD names no branch, so "has this landed" has no answer. The
  // bare host is the exception: it is detached by construction and never holds
  // work of its own.
  if (!bare && !branch) {
    return {
      sweep: false,
      reason: "detached HEAD: nothing names what this holds",
    };
  }
  if (!bare && !landed) {
    return { sweep: false, reason: "commits have not landed on origin/main" };
  }
  if (!bare && branch && openPrBranches.has(branch)) {
    // Landed and still open means a follow-up commit is expected: rebuilding
    // from cold to answer a review comment is a cost this tool imposed.
    return {
      sweep: false,
      reason: "an open pull request still points at this branch",
    };
  }
  if (owner && session && owner !== session) {
    return { sweep: false, reason: `owned by session ${owner.slice(0, 12)}` };
  }
  if (typeof modifiedAt !== "number") {
    // Unreadable mtime is treated as active. Guessing "old" from a failure is
    // how a sweep deletes the cache of the build that is running.
    return { sweep: false, reason: "age could not be read" };
  }
  const ageMs = now - modifiedAt;
  if (ageMs < graceMs) {
    return { sweep: false, reason: "inside the recent-activity grace period" };
  }
  if (ageMs < minimumAgeMs) {
    return { sweep: false, reason: "newer than the age threshold" };
  }
  return { sweep: true, reason: "regenerable and idle" };
}

/**
 * The sweep plan, and everything refused with the rule that refused it.
 *
 * Oldest first, so a disk budget keeps the caches most likely to be reused
 * warm and takes the ones nobody has touched. With no budget the plan is
 * everything eligible; with one, it stops as soon as the projected total falls
 * under it, which is what lets this run continuously without fighting the
 * builds people are actually doing.
 *
 * Shared by dry-run and apply, so what a report promises is exactly what an
 * apply performs — the two cannot drift into disagreeing.
 */
export function planArtefactSweep(candidates, options = {}) {
  const { budgetBytes = null } = options;
  const plan = [];
  const skipped = [];

  const eligible = [];
  for (const candidate of candidates) {
    const verdict = classifyArtefact(candidate, options);
    if (verdict.sweep) eligible.push(candidate);
    else skipped.push({ ...candidate, reason: verdict.reason });
  }

  eligible.sort((a, b) => a.modifiedAt - b.modifiedAt);

  const total = candidates.reduce((sum, c) => sum + (c.bytes ?? 0), 0);
  let remaining = total;
  for (const candidate of eligible) {
    if (budgetBytes !== null && remaining <= budgetBytes) {
      skipped.push({ ...candidate, reason: "disk is already under budget" });
      continue;
    }
    plan.push(candidate);
    remaining -= candidate.bytes ?? 0;
  }

  return {
    plan,
    skipped,
    bytes: plan.reduce((sum, c) => sum + (c.bytes ?? 0), 0),
    files: plan.reduce((sum, c) => sum + (c.files ?? 0), 0),
  };
}

/**
 * Whether a worktree can be reclaimed, and if not, which rule stopped it.
 *
 * Pure: every fact is gathered by the caller, so each refusal is testable
 * without creating or destroying a worktree. That matters more here than
 * usual — a cleanup tool tested only on what it deletes has not been tested
 * on what it must refuse to delete.
 */
export function classifyWorktree(
  worktree,
  {
    session,
    liveClaimBranches = new Set(),
    claimStateAvailable = true,
    abandonedBranches = new Set(),
  } = {},
) {
  const { path, branch, bare, dirty, landed, owner, building, current } =
    worktree;

  if (bare) {
    return {
      reclaim: false,
      reason: "the primary checkout hosts every worktree",
    };
  }
  if (current) {
    // Found on the first live run: this tool's own worktree was merged, clean
    // and idle, so every other rule said yes. Reclaiming it would delete the
    // target/ directory out from under the running process and then try to
    // remove the checkout it is executing in. A cleanup tool must not be its
    // own first casualty.
    return { reclaim: false, reason: "this tool is running here" };
  }
  if (!branch) {
    // A detached HEAD names no branch, so "has it landed" has no answer here.
    // Answering it anyway is how a cleanup tool deletes something it never read.
    return {
      reclaim: false,
      reason: "detached HEAD: nothing names what this holds",
    };
  }
  if (PROTECTED_BRANCHES.has(branch)) {
    return { reclaim: false, reason: `${branch} is protected` };
  }
  if (!claimStateAvailable) {
    return {
      reclaim: false,
      reason: "authoritative claim state is unavailable",
    };
  }
  if (liveClaimBranches.has(branch)) {
    return { reclaim: false, reason: "held by a live Lodestar claim" };
  }
  if (dirty) {
    // Untracked counts. A file nobody has staged yet is the most valuable thing
    // in the tree, not the least, and it exists nowhere else.
    return { reclaim: false, reason: "uncommitted or untracked changes" };
  }
  if (building) {
    return { reclaim: false, reason: "a build is running here" };
  }
  if (owner && session && owner !== session) {
    return { reclaim: false, reason: `owned by session ${owner.slice(0, 12)}` };
  }
  if (!landed && !abandonedBranches.has(branch)) {
    return { reclaim: false, reason: "commits have not landed on origin/main" };
  }
  // Abandoned and merged are both finished, and the operator is told which,
  // because "reclaimed" reads very differently when the work was thrown away.
  return {
    reclaim: true,
    reason: landed
      ? "merged and idle"
      : "abandoned: its pull request closed unmerged",
    path,
    branch,
  };
}

/**
 * Branches whose pull request was closed without merging.
 *
 * Such a branch will never land, so the `!landed` rule above would keep its
 * worktree forever — and the more disciplined the fleet is about closing
 * duplicate pull requests, the more dead worktrees it accumulates. That is the
 * measured failure mode in the header comment, reached by doing the right thing.
 *
 * Fails to the EMPTY set, which is the opposite of `readOpenPrBranches` in
 * artefact-sweep.mjs and deliberately so: that set PROTECTS, so not knowing must
 * refuse; this set PERMITS, so not knowing must permit nothing. Both directions
 * are "assume nothing was proven", which is only the same word, not the same
 * value.
 */
export function readAbandonedBranches() {
  try {
    const out = execFileSync(
      "gh",
      [
        "pr",
        "list",
        "--state",
        "closed",
        "--limit",
        "100",
        "--json",
        "headRefName,mergedAt",
      ],
      { encoding: "utf8", env: { ...process.env, GH_PAGER: "cat" } },
    );
    return new Set(
      JSON.parse(out)
        .filter((pr) => !pr.mergedAt)
        .map((pr) => pr.headRefName),
    );
  } catch {
    return new Set();
  }
}

/**
 * Whether a kept worktree holds only work that already exists on origin/main,
 * and is therefore worth showing a PERSON. Never a reclaim decision.
 *
 * The refusal that keeps these trees — "uncommitted or untracked changes" — is
 * correct and is deliberately left firing. Its defect is that it has no exit:
 * untracked files never become clean on their own, so every audit declines the
 * same tree for the same reason forever, and the kept set only ever grows.
 *
 * This reports facts and stops. It does not decide that somebody's uncommitted
 * work is worthless, because that judgement rests on a content comparison with
 * no reliable basis and an outcome nobody can undo. Measured on the live case:
 * every uncommitted path was an EARLIER, SMALLER draft of a file since shipped
 * (99 lines against 247), so a byte-equality test would have reported nothing.
 * Presence on origin/main is the honest signal; supersession is the person's
 * call.
 */
export function classifySuperseded({
  dirty,
  landed,
  dirtyPaths = [],
  unmatchedPaths = [],
} = {}) {
  if (!dirty) return { report: false, reason: "nothing uncommitted" };
  if (!landed) {
    return { report: false, reason: "commits have not landed on origin/main" };
  }
  if (!dirtyPaths.length) {
    return { report: false, reason: "no uncommitted paths to compare" };
  }
  // One unmatched path is enough. A tree holding anything that exists nowhere
  // upstream contains new work, whatever the rest of it duplicates.
  if (unmatchedPaths.length) {
    return {
      report: false,
      reason: `${unmatchedPaths.length} uncommitted path(s) exist nowhere on origin/main`,
    };
  }
  return {
    report: true,
    reason: "every uncommitted path exists on origin/main",
  };
}

/** Uncommitted paths in a worktree: modified and untracked, files not folders. */
export function readDirtyPaths(worktreePath) {
  const modified = tryGit(["diff", "--name-only"], worktreePath);
  const untracked = tryGit(
    ["ls-files", "--others", "--exclude-standard"],
    worktreePath,
  );
  if (!modified.ok || !untracked.ok) return null;
  return [...modified.out.split(/\n/), ...untracked.out.split(/\n/)]
    .map((line) => line.trim())
    .filter(Boolean);
}

/** Of `paths`, those that exist nowhere on origin/main. */
export function unmatchedOnMain(paths, worktreePath) {
  // Missing is the expected answer for half these probes, and git reports it on
  // stderr. Left unsilenced, a clean report reads as a screen of fatal errors.
  const onMain = (path) => {
    try {
      execFileSync("git", ["cat-file", "-e", `origin/main:${path}`], {
        cwd: worktreePath,
        stdio: "ignore",
      });
      return true;
    } catch {
      return false;
    }
  };
  return paths.filter((path) => !onMain(path));
}

/** Branches held by unexpired task claims at this instant. */
export function liveClaimBranches(tasks, now = Math.floor(Date.now() / 1000)) {
  return new Set(
    tasks
      .filter(
        (task) =>
          task.status === "claimed" &&
          typeof task.branch === "string" &&
          task.branch.length > 0 &&
          Number.isInteger(task.lease_expires_at) &&
          task.lease_expires_at > now,
      )
      .map((task) => task.branch),
  );
}

/** Read the authoritative live-claim branch set, preserving failures as data. */
export function readLiveClaimState(
  repoRoot,
  {
    now = Math.floor(Date.now() / 1000),
    resolveServerFn = resolveServer,
    callToolsFn = callTools,
  } = {},
) {
  const server = resolveServerFn(repoRoot, "lodestar");
  if (!server) {
    return {
      available: false,
      branches: new Set(),
      reason: "no lodestar-mcp binary found",
    };
  }
  try {
    const [response] = callToolsFn(server, repoRoot, [
      {
        name: "task_query",
        // liveClaimBranches below only reads status/branch/lease_expires_at.
        arguments: { view: "board", include_terminal: false, detail: false },
      },
    ]);
    const tasks = Array.isArray(response) ? response : response?.tasks;
    if (!Array.isArray(tasks)) {
      return {
        available: false,
        boardUnreadable: true,
        branches: new Set(),
        reason: "Lodestar task_query did not return a board",
      };
    }
    return {
      available: true,
      branches: liveClaimBranches(tasks, now),
      reason: null,
    };
  } catch (error) {
    return {
      available: false,
      branches: new Set(),
      reason: `could not read the Lodestar board: ${error.message}`,
    };
  }
}

/** Explain an unavailable claim state without inventing a live claim. */
export function claimStateRefusal(claimState) {
  const message = `worktree-reclaim: refusing to reclaim named worktrees because authoritative claim state is unavailable: ${claimState.reason}.`;
  return claimState.boardUnreadable
    ? `${message}\n  ${unreadableBoardGuidance}`
    : message;
}

/** Re-read claims at the destructive boundary, after the report was printed. */
export function revalidateBeforeReclaim(
  worktree,
  {
    session,
    readClaimState = () => ({ available: false, branches: new Set() }),
    readAbandoned = () => new Set(),
  } = {},
) {
  const state = readClaimState();
  return classifyWorktree(worktree, {
    session,
    liveClaimBranches: state.branches,
    claimStateAvailable: state.available,
    // Re-read rather than carried: a reopened pull request between the report
    // and here would otherwise cost its remote branch under --remote.
    abandonedBranches: readAbandoned(),
  });
}

/**
 * Whether a worktree git failed to remove is still actually there.
 *
 * `git worktree remove` deletes the tree's contents and its `.git` link before
 * unlinking the directory itself, so a lock on the directory fails the command
 * *after* the worktree is already dismantled. Treating that as a keep is what
 * orphaned three branches here: the entry was reported `kept`, so its branch
 * was never deleted, and the `git worktree prune` at the end of the run then
 * deregistered the gutted worktree — putting the branch permanently beyond the
 * reach of a tool that only ever looks at registered worktrees.
 *
 * A refusal that happens *before* git touches anything (a dirty tree, say)
 * leaves the `.git` link in place, and that is a real keep.
 */
export function worktreeSurvivedRemoval(path, exists = existsSync) {
  return exists(join(path, ".git"));
}

/**
 * Remove one already-revalidated entry's build output, then (unless
 * `artifactsOnly`) the worktree and its branch.
 *
 * A directory that still will not clear after `removeTreeSafely`'s own
 * retries is reported and left in place, and nothing about that entry's
 * worktree is touched -- the caller moves on to the next entry rather than
 * letting one locked path abort a run with other, independent worktrees
 * still queued behind it.
 */
export function reclaimEntry(
  entry,
  {
    anchor,
    remote = false,
    artifactsOnly = false,
    rm = removeTreeSafely,
    git: gitFn = tryGit,
    exists = existsSync,
  } = {},
) {
  const { path, branch } = entry.worktree;
  for (const artifact of entry.artifacts) {
    const result = rm(artifact.dir);
    if (!result.ok) {
      const detail =
        result.error?.code ?? result.error?.message ?? "unknown error";
      return {
        reclaimed: false,
        reason: `${artifact.dir} would not clear (${detail}); leave it and retry later`,
      };
    }
  }
  if (artifactsOnly) {
    return { reclaimed: true, artifactsOnly: true };
  }
  const removed = gitFn(["worktree", "remove", path], anchor);
  let residue = null;
  if (!removed.ok) {
    if (worktreeSurvivedRemoval(path, exists)) {
      return { reclaimed: false, reason: removed.out.trim().split(/\r?\n/)[0] };
    }
    // Dismantled, only the final unlink failed. Retry it with the same helper
    // the artifacts use, then finish the reclaim either way: the branch must
    // not survive a worktree that is already gone.
    const unlinked = rm(path);
    if (!unlinked.ok && exists(path)) {
      const detail =
        unlinked.error?.code ?? unlinked.error?.message ?? "unknown error";
      residue = `${path} is an empty directory another process still holds (${detail}); remove it later`;
    }
  }
  gitFn(["branch", "-D", branch], anchor);
  if (remote) {
    gitFn(["push", "origin", "--delete", branch], anchor);
  }
  return { reclaimed: true, artifactsOnly: false, residue };
}

/**
 * Whether every commit on `branch` has landed on `base`, by patch equivalence.
 *
 * NOT `git merge-base --is-ancestor`. A squash or rebase merge lands every line
 * under a new commit id, so ancestry answers "no" for work that is fully
 * merged — an agent here already used ancestry, concluded 245 merged lines were
 * lost, and queued a PR to restore code that was already present. `git cherry`
 * marks a commit `-` when an equivalent patch is upstream and `+` when it is
 * genuinely absent, which is the question actually being asked.
 */
export function hasLanded(cherryOutput) {
  return cherryOutput
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .every((line) => !line.startsWith("+"));
}

/** Bytes and file count under a directory, following no symlinks. */
export function measureTree(root) {
  let bytes = 0;
  let files = 0;
  const stack = [root];
  while (stack.length) {
    let entries;
    const current = stack.pop();
    try {
      entries = readdirSync(current, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      if (entry.isSymbolicLink()) continue;
      const full = join(current, entry.name);
      if (entry.isDirectory()) {
        stack.push(full);
        continue;
      }
      try {
        bytes += statSync(full).size;
        files += 1;
      } catch {
        // A file that vanished between listing and stat is a build writing
        // underneath us; it is not ours to account for and not worth failing on.
      }
    }
  }
  return { bytes, files };
}

export const formatBytes = (bytes) => `${(bytes / 1024 ** 3).toFixed(2)} GiB`;

const git = (args, cwd) =>
  execFileSync("git", args, {
    cwd,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });

const tryGit = (args, cwd) => {
  try {
    return { ok: true, out: git(args, cwd) };
  } catch (error) {
    return { ok: false, out: String(error.stderr ?? error.message ?? "") };
  }
};

/** Read the worktree list as records, including the bare primary. */
export function readWorktrees(anchor) {
  return git(["worktree", "list", "--porcelain"], anchor)
    .split(/\r?\n\r?\n/)
    .map((block) => block.split(/\r?\n/).filter(Boolean))
    .filter((lines) => lines.length)
    .map((lines, index) => ({
      path: lines
        .find((l) => l.startsWith("worktree "))
        ?.slice("worktree ".length),
      branch:
        lines
          .find((l) => l.startsWith("branch refs/heads/"))
          ?.slice("branch refs/heads/".length) ?? null,
      bare: lines.includes("bare"),
      // git lists the main worktree first; the linked ones follow. This is the
      // checkout the fleet's scripts resolve their server binaries from.
      primary: index === 0,
    }))
    .filter((worktree) => worktree.path);
}

function gatherFacts(worktree, anchor) {
  // Compared before the cheap exits: the tool can be run from the primary
  // checkout or from a detached worktree too, and "am I standing here" must be
  // answered for those as well.
  const current = resolve(worktree.path) === resolve(anchor);
  if (worktree.bare || !worktree.branch) return { ...worktree, current };
  const status = tryGit(
    ["status", "--porcelain", "--untracked-files=normal"],
    worktree.path,
  );
  const cherry = tryGit(["cherry", "origin/main", worktree.branch], anchor);
  const gitDir = tryGit(["rev-parse", "--absolute-git-dir"], worktree.path);
  const markerPath = gitDir.ok ? join(gitDir.out.trim(), MARKER_NAME) : null;
  const dirty = !status.ok || status.out.trim().length > 0;
  // Only gathered for trees that are already being kept, so the extra git calls
  // never run on the trees this tool might act on.
  const dirtyPaths = dirty ? readDirtyPaths(worktree.path) : [];
  const behind = tryGit(
    ["rev-list", "--count", "HEAD..origin/main"],
    worktree.path,
  );
  return {
    ...worktree,
    current,
    // A status that cannot be read is treated as dirty. Guessing "clean" from a
    // failure is how a cleanup tool deletes a tree it could not inspect.
    dirty,
    dirtyPaths: dirtyPaths ?? [],
    unmatchedPaths: dirtyPaths?.length
      ? unmatchedOnMain(dirtyPaths, worktree.path)
      : [],
    behind: behind.ok ? Number(behind.out.trim()) : null,
    landed: cherry.ok && hasLanded(cherry.out),
    owner:
      markerPath && existsSync(markerPath)
        ? readFileSync(markerPath, "utf8").trim()
        : null,
    building: existsSync(join(worktree.path, "target", ".cargo-lock")),
  };
}

function main() {
  const argv = process.argv.slice(2);
  const reclaim = argv.includes("--reclaim");
  const remote = argv.includes("--remote");
  const artifactsOnly = argv.includes("--artifacts-only");
  const session = process.env.LODESTAR_SESSION_ID ?? null;
  const anchor = process.cwd();

  const fetched = tryGit(["fetch", "origin", "--quiet", "--prune"], anchor);
  const scriptDiff = fetched.ok
    ? tryGit(
        [
          "diff",
          "--quiet",
          "origin/main",
          "--",
          "scripts/worktree-reclaim.mjs",
        ],
        anchor,
      )
    : { ok: false };
  const freshness = reclaimScriptFreshness({
    fetched: fetched.ok,
    matchesOrigin: scriptDiff.ok,
  });
  if (!freshness.current) {
    console.error(
      `worktree-reclaim: refusing to run because ${freshness.reason}. Run it from a current checkout.`,
    );
    return;
  }

  const claimState = readLiveClaimState(anchor);
  if (!claimState.available) {
    console.error(claimStateRefusal(claimState));
  }
  const abandonedBranches = readAbandonedBranches();
  const worktrees = readWorktrees(anchor).map((worktree) =>
    gatherFacts(worktree, anchor),
  );
  const verdicts = worktrees.map((worktree) => ({
    worktree,
    verdict: classifyWorktree(worktree, {
      session,
      liveClaimBranches: claimState.branches,
      claimStateAvailable: claimState.available,
      abandonedBranches,
    }),
  }));

  const reclaimable = verdicts.filter((entry) => entry.verdict.reclaim);
  const kept = verdicts.filter((entry) => !entry.verdict.reclaim);

  let totalBytes = 0;
  for (const entry of reclaimable) {
    entry.artifacts = ARTIFACT_DIRECTORIES.map((rel) =>
      join(entry.worktree.path, rel),
    )
      .filter((dir) => existsSync(dir))
      .map((dir) => ({ dir, ...measureTree(dir) }));
    entry.bytes = entry.artifacts.reduce((sum, a) => sum + a.bytes, 0);
    totalBytes += entry.bytes;
  }

  console.log(
    `worktree-reclaim: ${reclaimable.length} reclaimable, ${kept.length} kept`,
  );
  for (const entry of reclaimable) {
    console.log(
      `  reclaim  ${entry.worktree.branch}  ${formatBytes(entry.bytes)}`,
    );
  }
  // Kept trees are listed with the rule that kept them, so a worktree nobody
  // reclaimed does not read like one the tool failed to notice.
  for (const entry of kept) {
    console.log(
      `  keep     ${entry.worktree.branch ?? "(detached)"}  — ${entry.verdict.reason}`,
    );
  }
  console.log(
    `worktree-reclaim: ${formatBytes(totalBytes)} of build output in reclaimable worktrees`,
  );

  // Ownership is the guard that holds back the most, and a total nobody can see
  // reads as "there is nothing left". Counted rather than measured: sizing them
  // means walking every target/ the tool is refusing to touch, which is minutes
  // of disk for a number the run cannot act on anyway.
  const heldByOthers = kept.filter((entry) =>
    entry.verdict.reason.startsWith("owned by session"),
  );
  if (heldByOthers.length) {
    console.log(
      `worktree-reclaim: ${heldByOthers.length} more are held by other sessions. ` +
        "Their owner reclaims them, or scripts/worktree-owner.mjs --adopt-worktree " +
        "transfers one whose session is provably gone.",
    );
  }

  // Printed after the kept list and never merged into it: a tree named here is
  // still kept, and the only thing that may act on it is a person.
  const superseded = kept.filter(
    (entry) => classifySuperseded(entry.worktree).report,
  );
  if (superseded.length) {
    console.log(
      `worktree-reclaim: ${superseded.length} kept worktree(s) hold nothing that is not already on origin/main:`,
    );
    for (const { worktree } of superseded) {
      const behind =
        worktree.behind === null ? "unknown" : `${worktree.behind} commits`;
      console.log(
        `  review   ${worktree.branch}  — ${behind} behind, ` +
          `${worktree.dirtyPaths.length} uncommitted path(s), each already on origin/main`,
      );
    }
    console.log(
      "worktree-reclaim: nothing above was touched. Open one, confirm its " +
        "uncommitted work is genuinely obsolete, and clear it yourself — this " +
        "tool will not decide that for you.",
    );
  }

  if (!reclaim) {
    console.log("worktree-reclaim: reporting only; pass --reclaim to act");
    return;
  }

  for (const entry of reclaimable) {
    const { branch } = entry.worktree;
    const refreshed = revalidateBeforeReclaim(entry.worktree, {
      session,
      readClaimState: () => readLiveClaimState(anchor),
      readAbandoned: readAbandonedBranches,
    });
    if (!refreshed.reclaim) {
      console.log(`worktree-reclaim: kept ${branch} — ${refreshed.reason}`);
      continue;
    }
    const result = reclaimEntry(entry, { anchor, remote, artifactsOnly });
    if (!result.reclaimed) {
      console.log(`worktree-reclaim: kept ${branch} — ${result.reason}`);
      continue;
    }
    console.log(
      result.artifactsOnly
        ? `worktree-reclaim: cleaned ${branch} (${formatBytes(entry.bytes)})`
        : `worktree-reclaim: reclaimed ${branch} (${formatBytes(entry.bytes)})`,
    );
    if (result.residue) {
      console.log(`worktree-reclaim: residue — ${result.residue}`);
    }
  }
  tryGit(["worktree", "prune"], anchor);
}

if (import.meta.filename === process.argv[1]) {
  main();
}
