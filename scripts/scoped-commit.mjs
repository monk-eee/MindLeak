// Commit-scope guard (ADR-0018). Stages and commits ONLY the paths you declare,
// via `git add -- <paths>` + `git commit -- <paths>` (pathspec) — never
// `git add -A` / `commit -a`. Worktree isolation prevents cross-agent staging;
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

import { execFileSync } from "node:child_process";

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

// Stage only the declared paths.
run(["add", "--", ...paths]);

// Report any pre-existing staged paths outside the declared set — the pathspec
// commit below leaves them untouched (they are NOT committed).
const staged = capture(["diff", "--cached", "--name-only"])
  .split("\n")
  .map((s) => s.trim())
  .filter(Boolean);
const foreign = staged.filter((file) => !isDeclared(file));
if (foreign.length) {
  console.warn(
    "scoped-commit: note — these staged paths are not yours and will be left uncommitted:",
  );
  for (const file of foreign) console.warn(`  ${file}`);
}

try {
  run(["commit", ...messageArgs, "--", ...paths]);
} catch (err) {
  process.exit(typeof err.status === "number" ? err.status : 1);
}
