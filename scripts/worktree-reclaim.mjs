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
import {
  existsSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
} from "node:fs";
import { join } from "node:path";

import { MARKER_NAME } from "./worktree-owner.mjs";

/** Branches that are never reclaimed whatever else is true of them. */
export const PROTECTED_BRANCHES = new Set(["main", "master"]);

/** Directories that hold build output and are never hand-edited. */
export const ARTIFACT_DIRECTORIES = [
  "target",
  "editors/vscode/node_modules",
  "editors/vscode/out",
  "editors/vscode/coverage",
  "editors/vscode/.vscode-test",
];

/**
 * Whether a worktree can be reclaimed, and if not, which rule stopped it.
 *
 * Pure: every fact is gathered by the caller, so each refusal is testable
 * without creating or destroying a worktree. That matters more here than
 * usual — a cleanup tool tested only on what it deletes has not been tested
 * on what it must refuse to delete.
 */
export function classifyWorktree(worktree, { session } = {}) {
  const { path, branch, bare, dirty, landed, owner, building } = worktree;

  if (bare) {
    return {
      reclaim: false,
      reason: "the primary checkout hosts every worktree",
    };
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
  if (!landed) {
    return { reclaim: false, reason: "commits have not landed on origin/main" };
  }
  return { reclaim: true, reason: "merged and idle", path, branch };
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
    .map((lines) => ({
      path: lines
        .find((l) => l.startsWith("worktree "))
        ?.slice("worktree ".length),
      branch:
        lines
          .find((l) => l.startsWith("branch refs/heads/"))
          ?.slice("branch refs/heads/".length) ?? null,
      bare: lines.includes("bare"),
    }))
    .filter((worktree) => worktree.path);
}

function gatherFacts(worktree, anchor) {
  if (worktree.bare || !worktree.branch) return worktree;
  const status = tryGit(
    ["status", "--porcelain", "--untracked-files=normal"],
    worktree.path,
  );
  const cherry = tryGit(["cherry", "origin/main", worktree.branch], anchor);
  const gitDir = tryGit(["rev-parse", "--absolute-git-dir"], worktree.path);
  const markerPath = gitDir.ok ? join(gitDir.out.trim(), MARKER_NAME) : null;
  return {
    ...worktree,
    // A status that cannot be read is treated as dirty. Guessing "clean" from a
    // failure is how a cleanup tool deletes a tree it could not inspect.
    dirty: !status.ok || status.out.trim().length > 0,
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

  tryGit(["fetch", "origin", "--quiet", "--prune"], anchor);

  const worktrees = readWorktrees(anchor).map((worktree) =>
    gatherFacts(worktree, anchor),
  );
  const verdicts = worktrees.map((worktree) => ({
    worktree,
    verdict: classifyWorktree(worktree, { session }),
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

  if (!reclaim) {
    console.log("worktree-reclaim: reporting only; pass --reclaim to act");
    return;
  }

  for (const entry of reclaimable) {
    const { path, branch } = entry.worktree;
    for (const artifact of entry.artifacts) {
      rmSync(artifact.dir, { recursive: true, force: true });
    }
    if (artifactsOnly) {
      console.log(
        `worktree-reclaim: cleaned ${branch} (${formatBytes(entry.bytes)})`,
      );
      continue;
    }
    const removed = tryGit(["worktree", "remove", path], anchor);
    if (!removed.ok) {
      console.log(
        `worktree-reclaim: kept ${branch} — ${removed.out.trim().split(/\r?\n/)[0]}`,
      );
      continue;
    }
    tryGit(["branch", "-D", branch], anchor);
    if (remote) {
      tryGit(["push", "origin", "--delete", branch], anchor);
    }
    console.log(
      `worktree-reclaim: reclaimed ${branch} (${formatBytes(entry.bytes)})`,
    );
  }
  tryGit(["worktree", "prune"], anchor);
}

if (import.meta.filename === process.argv[1]) {
  main();
}
