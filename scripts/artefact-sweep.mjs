// Continuous build-artefact hygiene: the sweep that keeps the fleet's disk
// bounded without anybody remembering to run it.
//
// A CLI does not solve this. The agent that filled a cache has finished and
// moved on by the time it is safe to delete, so the mess is always somebody
// else's and it grows every time the fleet works correctly. Measured
// 2026-07-30: 149.18 GiB across 1,006,187 files in 124 reproducible cache
// directories, all of them on clean branches already ancestral to origin/main.
// A tool nobody invokes reclaimed none of it.
//
// So this runs from the delivery watcher, which is already persistent and
// already single-owner. Three properties make that safe to do unattended:
//
//   1. ONE SWEEPER. The lock lives in the common Git directory, which every
//      worktree shares, so two watchers in two worktrees cannot sweep at once.
//      `mkdir` is the primitive because it is atomic on every platform -- a
//      check-then-write would race exactly when the fleet is busiest.
//   2. BOUNDED. A persisted last-run timestamp means restarting the watcher
//      does not restart the work, and the interval keeps a sweep off the hot
//      path of the queue it shares a process with.
//   3. REVALIDATED. Eligibility is re-evaluated immediately before each
//      deletion, because a plan is a statement about the past: a tree that was
//      idle when the walk started can be building by the time the walk ends.
//
// Selection itself lives in worktree-reclaim.mjs and is imported, not copied --
// one definition of what may be deleted, exercised by both the CLI and this.
//
// Platform-agnostic: node + git only.

import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";

import {
  ARTEFACT_KINDS,
  classifyArtefact,
  formatBytes,
  measureTree,
  planArtefactSweep,
  readWorktrees,
} from "./worktree-reclaim.mjs";

/** How often an unattended sweep runs. Long: this is housekeeping, not work. */
export const SWEEP_INTERVAL_MS = 6 * 60 * 60 * 1000;

/** A lock older than this is treated as abandoned by a killed process. */
export const LOCK_STALE_MS = 30 * 60 * 1000;

const LOCK_DIR = "artefact-sweep.lock";
const STATE_FILE = "artefact-sweep.json";

/**
 * Whether a sweep is due.
 *
 * Pure. A missing or unreadable timestamp counts as due, so the first run after
 * an upgrade audits rather than waiting a full interval to discover it should
 * have.
 */
export function dueForSweep(lastRunAt, now, intervalMs = SWEEP_INTERVAL_MS) {
  if (typeof lastRunAt !== "number" || Number.isNaN(lastRunAt)) return true;
  return now - lastRunAt >= intervalMs;
}

export function readSweepState(commonDir) {
  try {
    return JSON.parse(readFileSync(join(commonDir, STATE_FILE), "utf8"));
  } catch {
    // Absent or corrupt reads as "never run", which is the safe direction: an
    // extra audit costs a disk walk, a skipped one costs unbounded growth.
    return { lastRunAt: null };
  }
}

export function writeSweepState(commonDir, state) {
  writeFileSync(
    join(commonDir, STATE_FILE),
    `${JSON.stringify(state, null, 2)}\n`,
    "utf8",
  );
}

/**
 * Take the fleet-wide sweep lock, or report who holds it.
 *
 * `mkdir` fails if the directory exists, atomically, which is the whole
 * mechanism. A stale lock is broken only on age, never on "the pid looks gone":
 * pid reuse makes that check wrong precisely when it matters.
 */
export function acquireSweepLock(
  commonDir,
  now = Date.now(),
  staleMs = LOCK_STALE_MS,
) {
  const dir = join(commonDir, LOCK_DIR);
  try {
    mkdirSync(dir);
  } catch {
    let age = null;
    try {
      age = now - statSync(dir).mtimeMs;
    } catch {
      return { held: false, reason: "another sweep holds the lock" };
    }
    if (age < staleMs) {
      return {
        held: false,
        reason: `another sweep has held the lock for ${Math.round(age / 1000)}s`,
      };
    }
    rmSync(dir, { recursive: true, force: true });
    try {
      mkdirSync(dir);
    } catch {
      return { held: false, reason: "another sweep took the lock first" };
    }
  }
  return {
    held: true,
    release: () => rmSync(dir, { recursive: true, force: true }),
  };
}

/**
 * Execute a plan, re-checking each entry immediately before deleting it.
 *
 * Side effects are injected so the contract that matters -- that apply removes
 * the planned artefact directory and nothing else -- is provable without a
 * disk. `revalidate` returns the current facts for one candidate; returning
 * null means they could not be read, which fails closed.
 */
export function applySweep(
  plan,
  { revalidate, remove, now = Date.now(), options = {} },
) {
  const removed = [];
  const abandoned = [];
  for (const candidate of plan) {
    const fresh = revalidate(candidate);
    if (!fresh) {
      abandoned.push({ ...candidate, reason: "facts could not be re-read" });
      continue;
    }
    const verdict = classifyArtefact(fresh, { ...options, now });
    if (!verdict.sweep) {
      // The world moved while the walk ran. That is expected, not an error.
      abandoned.push({
        ...candidate,
        reason: `no longer eligible: ${verdict.reason}`,
      });
      continue;
    }
    remove(candidate.path);
    removed.push(candidate);
  }
  return {
    removed,
    abandoned,
    bytes: removed.reduce((sum, c) => sum + (c.bytes ?? 0), 0),
    files: removed.reduce((sum, c) => sum + (c.files ?? 0), 0),
  };
}

/** Skipped entries collapsed into counts, so a report is readable at a glance. */
export function summariseSkips(skipped) {
  const counts = new Map();
  for (const entry of skipped)
    counts.set(entry.reason, (counts.get(entry.reason) ?? 0) + 1);
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(([reason, count]) => ({ reason, count }));
}

const git = (args, cwd) =>
  execFileSync("git", args, {
    cwd,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    // Capture stderr rather than inheriting it. Several of these commands fail
    // as a matter of course -- `git status` in the bare host, `git cherry` for a
    // branch that has since been deleted -- and each failure is already handled
    // as a refusal. Letting git print `fatal:` anyway makes a working sweep read
    // like a broken one.
    stdio: ["ignore", "pipe", "pipe"],
  });

const tryGit = (args, cwd) => {
  try {
    return { ok: true, out: git(args, cwd) };
  } catch (error) {
    return { ok: false, out: String(error.stderr ?? error.message ?? "") };
  }
};

/** Branches with an open pull request, or null when GitHub cannot be reached. */
export function readOpenPrBranches() {
  try {
    const out = execFileSync(
      "gh",
      [
        "pr",
        "list",
        "--state",
        "open",
        "--limit",
        "100",
        "--json",
        "headRefName",
      ],
      {
        encoding: "utf8",
        env: { ...process.env, GH_PAGER: "cat" },
      },
    );
    return new Set(JSON.parse(out).map((pr) => pr.headRefName));
  } catch {
    // Fail closed: an unknown PR set must not read as "no branch has a PR".
    return null;
  }
}

/** Facts for one worktree, shared by the walk and by revalidation. */
export function worktreeFacts(worktree, anchor) {
  // The bare host has no working tree, so `git status` there is an error rather
  // than an answer, and it holds no branch of its own to have landed.
  if (worktree.bare) {
    return { ...worktree, dirty: false, landed: true, building: false };
  }
  const status = tryGit(
    ["status", "--porcelain", "--untracked-files=normal"],
    worktree.path,
  );
  const cherry = worktree.branch
    ? tryGit(["cherry", "origin/main", worktree.branch], anchor)
    : { ok: false, out: "" };
  return {
    ...worktree,
    dirty: !status.ok || status.out.trim().length > 0,
    landed:
      cherry.ok &&
      !cherry.out.split(/\r?\n/).some((l) => l.trim().startsWith("+")),
    building: existsSync(join(worktree.path, "target", ".cargo-lock")),
  };
}

/**
 * The worktree a candidate path sits inside.
 *
 * Longest prefix wins, and the match must end on a separator. `MindLeak` is a
 * prefix of `MindLeak-artifactid` as a plain string, so a naive startsWith
 * attributes a sibling's cache to the bare host -- which is the one place a
 * misattribution would sweep target/release out from under the running servers.
 */
export function owningWorktree(worktrees, path) {
  const normalise = (p) => p.replace(/\\/g, "/").replace(/\/+$/, "");
  const target = normalise(path);
  let best = null;
  for (const worktree of worktrees) {
    const root = normalise(worktree.path);
    if (target !== root && !target.startsWith(`${root}/`)) continue;
    if (!best || root.length > normalise(best.path).length) best = worktree;
  }
  return best;
}

/** Every artefact directory that exists, with its size and age. */
export function gatherCandidates(anchor, { measure = measureTree } = {}) {
  const candidates = [];
  for (const worktree of readWorktrees(anchor)) {
    const facts = worktreeFacts(worktree, anchor);
    for (const { kind, rel } of ARTEFACT_KINDS) {
      const path = join(worktree.path, rel);
      if (!existsSync(path)) continue;
      let modifiedAt = null;
      try {
        modifiedAt = statSync(path).mtimeMs;
      } catch {
        modifiedAt = null;
      }
      candidates.push({
        ...facts,
        kind,
        rel,
        path,
        modifiedAt,
        ...measure(path),
      });
    }
  }
  return candidates;
}

/** One sweep: plan, report, and act only when asked to. */
export function sweep({
  anchor,
  apply = false,
  now = Date.now(),
  options = {},
}) {
  const openPrBranches = readOpenPrBranches();
  const session = process.env.LODESTAR_SESSION_ID ?? null;
  // A null PR set means GitHub was unreachable. Treating that as "no branch has
  // an open PR" would sweep exactly the caches a reviewer is about to need.
  if (openPrBranches === null) {
    return {
      skippedRun: "GitHub was unreachable, so open pull requests are unknown",
    };
  }

  const settings = { ...options, now, session, openPrBranches };
  const candidates = gatherCandidates(anchor);
  const { plan, skipped, bytes, files } = planArtefactSweep(
    candidates,
    settings,
  );

  const report = {
    planned: plan.length,
    bytes,
    files,
    skips: summariseSkips(skipped),
    candidates: candidates.length,
  };
  if (!apply) return { ...report, applied: null };

  const applied = applySweep(plan, {
    now,
    options: settings,
    revalidate: (candidate) => {
      const worktree = owningWorktree(readWorktrees(anchor), candidate.path);
      if (!worktree) return null;
      let modifiedAt = null;
      try {
        modifiedAt = statSync(candidate.path).mtimeMs;
      } catch {
        return null;
      }
      return {
        ...worktreeFacts(worktree, anchor),
        kind: candidate.kind,
        rel: candidate.rel,
        path: candidate.path,
        modifiedAt,
      };
    },
    remove: (path) => rmSync(path, { recursive: true, force: true }),
  });
  return { ...report, applied };
}

/**
 * The watcher's entry point: sweep when due, holding the fleet-wide lock, and
 * say what happened in one line so it does not drown the queue it shares.
 */
export function sweepIfDue({
  anchor,
  commonDir,
  apply = false,
  now = Date.now(),
  intervalMs = SWEEP_INTERVAL_MS,
  force = false,
  run = sweep,
}) {
  const state = readSweepState(commonDir);
  if (!force && !dueForSweep(state.lastRunAt, now, intervalMs))
    return { ran: false, reason: "not due" };

  const lock = acquireSweepLock(commonDir, now);
  if (!lock.held) return { ran: false, reason: lock.reason };

  try {
    const result = run({ anchor, apply, now });
    if (result.skippedRun) return { ran: false, reason: result.skippedRun };
    // Only an apply records the run. A dry run that marked itself done would
    // silently suppress the next real sweep for a whole interval, so asking
    // what would happen would stop anything happening.
    if (apply) writeSweepState(commonDir, { lastRunAt: now });
    return { ran: true, result };
  } finally {
    lock.release();
  }
}

export function describeSweep(result) {
  const acted = result.applied;
  const head = acted
    ? `artefact-sweep: reclaimed ${formatBytes(acted.bytes)} across ${acted.removed.length} directories (${acted.files} files)`
    : `artefact-sweep: ${formatBytes(result.bytes)} reclaimable across ${result.planned} of ${result.candidates} directories (reporting only)`;
  const skips = result.skips
    .map((s) => `    ${String(s.count).padStart(4)}  ${s.reason}`)
    .join("\n");
  const abandoned = acted?.abandoned.length
    ? `\n    ${acted.abandoned.length} abandoned after revalidation`
    : "";
  return skips ? `${head}\n${skips}${abandoned}` : `${head}${abandoned}`;
}
