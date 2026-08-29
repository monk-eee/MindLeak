// Commit-scope guard (ADR-0018). Stages and commits ONLY the paths you declare,
// via `git add -A -- <paths>` + `git commit -- <paths>` (pathspec) — never
// unscoped `git add -A` / `commit -a`. The explicit pathspec stages deletions
// without reaching outside the declared scope. Worktree isolation prevents
// cross-agent staging;
// this guard separately guarantees the current worktree cannot sweep unrelated
// staged paths into the wrong task or attribution. Any pre-existing staged path
// outside the declared set is reported and left uncommitted.
//
// It also refuses the pre-commit *stash race* (ADR-0038). `pre-commit` stashes
// every unstaged change before running hooks and restores it afterwards. That is
// safe alone and corrupting in a fleet: if another agent writes to the same
// working tree inside that window, the restore collides with their edit, hooks
// report the fictitious "files were modified by this hook", and the result is
// painful to unpick precisely because nothing names the real cause. So when a
// fleet is running, foreign unstaged work is a hard stop, not a warning.
//
// Platform-agnostic: git + node only. Usage:
//   node scripts/scoped-commit.mjs -m "<message>" -- <path> [<path> ...]
//   node scripts/scoped-commit.mjs -F <msgfile>  -- <path> [<path> ...]
//   ... --allow-foreign-wip   (single-operator escape hatch; see below)
//   ... --adopt-worktree      (deliberately take over a peer's worktree)

import { execFileSync } from "node:child_process";

import { readOwnClaims, uncoveredCommitNotice } from "./claim-gate.mjs";
import { checkWorktreeOwnership, refusalMessage } from "./worktree-owner.mjs";

const argv = process.argv.slice(2);
const bail = (message) => {
  console.error(`scoped-commit: ${message}`);
  process.exit(2);
};

const sep = argv.indexOf("--");
if (sep === -1) bail("missing `--` before the path list");
const opts = argv.slice(0, sep);
const paths = argv.slice(sep + 1).filter(Boolean);
if (paths.length === 0) bail("declare at least one path to commit");

const allowForeignWip = opts.includes("--allow-foreign-wip");

let messageArgs = [];
const mi = opts.indexOf("-m");
const fi = opts.indexOf("-F");
if (mi !== -1 && opts[mi + 1]) messageArgs = ["-m", opts[mi + 1]];
else if (fi !== -1 && opts[fi + 1]) messageArgs = ["-F", opts[fi + 1]];
else bail("provide a message with -m <message> or -F <file>");

const run = (a, o = {}) => execFileSync("git", a, { stdio: "inherit", ...o });
const capture = (a) => execFileSync("git", a, { encoding: "utf8" }).trim();

const declared = paths.map((p) => p.replace(/\\/g, "/").replace(/\/+$/, ""));
const isDeclared = (file) =>
  declared.some((d) => file === d || file.startsWith(`${d}/`));

// How many working trees are attached to this repository. More than one means a
// fleet is live.
const attachedWorktrees = () =>
  capture(["worktree", "list", "--porcelain"])
    .split("\n")
    .filter((line) => line.startsWith("worktree ")).length;

// Whether this is the primary checkout rather than a linked worktree. A linked
// worktree is yours alone, so unrelated unstaged work in it is your own and the
// stash is harmless. The primary checkout is the one everybody can reach for,
// and therefore the only place a second writer can appear mid-stash.
const isPrimaryCheckout = () => {
  const gitDir = capture(["rev-parse", "--absolute-git-dir"]).toLowerCase();
  const commonDir = capture([
    "rev-parse",
    "--path-format=absolute",
    "--git-common-dir",
  ])
    .toLowerCase()
    .replace(/\/$/, "");
  return gitDir === commonDir;
};

// Tracked files modified but not staged. These are exactly what pre-commit will
// stash and restore around the hook run.
const unstagedTracked = () =>
  capture(["diff", "--name-only"])
    .split("\n")
    .map((s) => s.trim())
    .filter(Boolean);

const worktrees = attachedWorktrees();
const foreignWip = unstagedTracked().filter((file) => !isDeclared(file));

if (
  worktrees > 1 &&
  isPrimaryCheckout() &&
  foreignWip.length > 0 &&
  !allowForeignWip
) {
  console.error(
    `scoped-commit: refusing to commit — this is the primary checkout, ${worktrees} working ` +
      `trees are attached, and ${foreignWip.length} unstaged file(s) outside your declared ` +
      `paths are live here:`,
  );
  for (const file of foreignWip.slice(0, 10)) console.error(`  ${file}`);
  if (foreignWip.length > 10) {
    console.error(`  ... and ${foreignWip.length - 10} more`);
  }
  console.error(
    "\npre-commit stashes those before running hooks and restores them after. If the\n" +
      "agent that owns them writes during that window, the restore collides and the\n" +
      "hooks report a fictitious failure that is very hard to trace back to here.\n" +
      "\nMove this work to your own worktree (ADR-0038):\n" +
      "  git worktree add ../MindLeak-<workstream> -b fleet/<workstream> origin/main\n" +
      "\nIf you are the only operator and those edits are yours, re-run with " +
      "--allow-foreign-wip.",
  );
  process.exit(3);
}

// Worktree ownership (ADR-0038). The guard above protects the primary checkout,
// but it assumed a linked worktree is yours by construction — and that
// assumption is the hole: nothing stopped an agent committing in a *peer's*
// worktree. See scripts/worktree-owner.mjs for why that is corrupting.
const ownership = checkWorktreeOwnership({
  adopt: opts.includes("--adopt-worktree"),
});

if (ownership.action === "refuse") {
  console.error(
    `scoped-commit: refusing to commit — ${refusalMessage(ownership)}`,
  );
  process.exit(4);
}

// Stage only the declared paths, including deletions. Plain `git add -- <path>`
// refuses a path that no longer exists even when that deletion is exactly what
// this scoped commit must preserve, and `git add -A` handles a fresh deletion
// but still refuses a path already gone from disk and staged.
//
// Paths that are already fully represented in the index and no longer exist on
// disk. `git add` refuses a pathspec matching nothing, so naming one of these
// aborts the whole staging step with `fatal: pathspec ... did not match any
// files` — an error that names your path and not the reason.
//
// Two ways a declared path gets into this state, and only the first was handled
// before: a plain staged delete (`D`), and the OLD side of a staged rename
// (`R`). The second is what `git mv old new` produces, and it is invisible to
// `--diff-filter=D` because git reports it as `R`, not `D`. Measured: declaring
// the old path of a file→module-directory split (`git mv daemon.rs
// daemon/mod.rs`, then committing `crates/.../daemon.rs`) failed with exit 128
// and no commit, for a rename already correctly staged.
//
// Both are already in the index, so there is nothing to add for them — they
// only need to appear in the commit pathspec below, which `commitPaths`
// handles.
const stagedRenames = () =>
  capture(["diff", "--cached", "--name-status", "-M"])
    .split("\n")
    .map((line) => line.trim().split("\t"))
    .filter(([status]) => status?.startsWith("R"))
    .map(([, from, to]) => ({ from, to }));

const alreadyStagedAndGone = new Set([
  ...capture(["diff", "--cached", "--name-only", "--diff-filter=D"])
    .split("\n")
    .map((path) => path.trim())
    .filter(Boolean),
  ...stagedRenames().map(({ from }) => from),
]);
const pathsToStage = paths.filter(
  (path) => !alreadyStagedAndGone.has(path.replace(/\\/g, "/")),
);
if (pathsToStage.length) {
  run(["add", "-A", "--", ...pathsToStage]);
}

// A staged rename (e.g. `git mv old new`, or a delete+add git's -M heuristic
// recognizes as similar enough) whose NEW side is declared needs its OLD side
// in the commit pathspec too: `git commit -- <pathspec>` reconstructs its
// tree from HEAD content for any path NOT named in the pathspec, including a
// path deleted in the index -- so an unlisted rename source silently
// resurrects the old file's HEAD content alongside the new one
// (gaps.d/scoped-commit-cannot-express-a-rename-whose-old-path.md). The OLD
// side can never go in the `git add` list above: it no longer exists on disk,
// and `git add` refuses a pathspec matching nothing.
// Recomputed after staging: `git add` above can itself create a rename that did
// not exist in the index a moment ago.
const renamedSources = stagedRenames()
  .filter(({ to }) => isDeclared(to))
  .map(({ from }) => from);
const isPartOfThisCommit = (file) =>
  isDeclared(file) || renamedSources.includes(file);
const commitPaths = [...new Set([...paths, ...renamedSources])];

// Report any pre-existing staged paths outside the declared set — the pathspec
// commit below leaves them untouched (they are NOT committed).
const staged = capture(["diff", "--cached", "--name-only"])
  .split("\n")
  .map((s) => s.trim())
  .filter(Boolean);
const foreign = staged.filter((file) => !isPartOfThisCommit(file));
if (foreign.length) {
  console.warn(
    "scoped-commit: note — these staged paths are not yours and will be left uncommitted:",
  );
  for (const file of foreign) console.warn(`  ${file}`);
}

// The claim advisory, printed BEFORE the commit exists (ADR-0048 keeps commits
// ungated; this is a warning, never a gate). Placed here rather than after the
// commit deliberately: a notice the reader sees only once the commit is made
// names a repair that does not exist, because a claim taken afterwards does not
// reach back over it. Here, stopping and claiming is still a real option.
const claimState = readOwnClaims({
  repoRoot: capture(["rev-parse", "--show-toplevel"]),
  sessionId: process.env.LODESTAR_SESSION_ID,
});
const claimNotice = uncoveredCommitNotice({
  tasks: claimState.tasks,
  agent: claimState.agent,
  now: Date.now() / 1000,
  reachable: claimState.reachable,
});
if (claimNotice) console.warn(`scoped-commit: ${claimNotice}`);

try {
  run(["commit", ...messageArgs, "--", ...commitPaths]);
} catch (err) {
  process.exit(typeof err.status === "number" ? err.status : 1);
}
