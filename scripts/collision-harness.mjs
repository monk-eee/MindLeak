// Deterministic collision harness (ADR-0018). Proves, in a throwaway sandbox repo
// that never touches the real one, the properties that make concurrent agents
// sharing one git object store safe:
//   (i)   two worktrees do not clobber each other's uncommitted files
//   (ii)  each worktree commits independently against its own index (no sweep)
//   (iii) a foreign/broken untracked file in worktree A is absent from worktree B
//         (so it cannot poison B's build or hooks)
//   (iv)  overlapping edits surface as an honest git merge conflict, not silent loss
// Plus a mode-agnostic check that scripts/scoped-commit.mjs behaves identically in
// the primary worktree and a linked worktree.
//
// Platform-agnostic: git + node only. Usage:  node scripts/collision-harness.mjs

import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const scopedCommit = join(scriptDir, "scoped-commit.mjs");

const sandbox = mkdtempSync(join(tmpdir(), "mindleak-collision-"));
const main = join(sandbox, "main");
const wtB = join(sandbox, "agent-b");
let failures = 0;

const git = (cwd, args) =>
  execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
const node = (cwd, args) =>
  execFileSync(process.execPath, args, { cwd, encoding: "utf8" });

function check(name, condition) {
  console.log(`${condition ? "PASS" : "FAIL"}  ${name}`);
  if (!condition) failures += 1;
}

try {
  // --- sandbox repo with one commit, then a second worktree -------------------
  execFileSync("git", ["init", "-q", "-b", "main", main], { stdio: "ignore" });
  git(main, ["config", "user.email", "harness@example.com"]);
  git(main, ["config", "user.name", "harness"]);
  git(main, ["config", "commit.gpgsign", "false"]);
  writeFileSync(join(main, "shared.txt"), "base\n");
  git(main, ["add", "shared.txt"]);
  git(main, ["commit", "-q", "-m", "base"]);
  git(main, ["worktree", "add", "-q", "--detach", wtB, "HEAD"]);

  // --- (i) no clobber of uncommitted files ------------------------------------
  writeFileSync(join(main, "a-only.txt"), "A working copy\n");
  writeFileSync(join(wtB, "b-only.txt"), "B working copy\n");
  check(
    "(i)   A's uncommitted file is invisible to worktree B",
    !existsSync(join(wtB, "a-only.txt")),
  );
  check(
    "(i)   B's uncommitted file is invisible to worktree A",
    !existsSync(join(main, "b-only.txt")),
  );

  // --- (iii) foreign/broken untracked file in A cannot poison B ---------------
  writeFileSync(join(main, "broken.rs"), "fn broken( {{{ not rust\n");
  check(
    "(iii) A's foreign untracked file never appears in worktree B",
    !existsSync(join(wtB, "broken.rs")),
  );

  // --- (ii) independent commits against separate indexes ----------------------
  git(main, ["add", "a-only.txt"]);
  git(main, ["commit", "-q", "-m", "A commit"]);
  git(wtB, ["add", "b-only.txt"]);
  git(wtB, ["commit", "-q", "-m", "B commit"]);
  const aFiles = git(main, ["show", "--name-only", "--format=", "HEAD"]);
  const bFiles = git(wtB, ["show", "--name-only", "--format=", "HEAD"]);
  check(
    "(ii)  A's commit contains only A's file (no sweep of B's work)",
    aFiles.includes("a-only.txt") && !aFiles.includes("b-only.txt"),
  );
  check(
    "(ii)  B's commit contains only B's file (no sweep of A's work)",
    bFiles.includes("b-only.txt") && !bFiles.includes("a-only.txt"),
  );

  // --- (iv) overlapping edits surface as an honest merge conflict -------------
  const base = git(main, ["rev-parse", "HEAD~1"]);
  git(main, ["checkout", "-q", "-B", "branch-a", base]);
  git(wtB, ["checkout", "-q", "-B", "branch-b", base]);
  writeFileSync(join(main, "shared.txt"), "A's version\n");
  git(main, ["commit", "-q", "-am", "A edits shared"]);
  writeFileSync(join(wtB, "shared.txt"), "B's version\n");
  git(wtB, ["commit", "-q", "-am", "B edits shared"]);
  let conflicted = false;
  try {
    git(main, ["merge", "--no-edit", "branch-b"]);
  } catch {
    conflicted = true;
  }
  const hasMarkers =
    existsSync(join(main, "shared.txt")) &&
    readFileSync(join(main, "shared.txt"), "utf8").includes("<<<<<<<");
  check(
    "(iv)  overlapping edits produce an honest merge conflict (no silent loss)",
    conflicted && hasMarkers,
  );
  try {
    git(main, ["merge", "--abort"]);
  } catch {
    /* nothing to abort */
  }

  // --- mode-agnostic: scoped-commit.mjs behaves the same in both worktrees -----
  for (const [label, cwd] of [
    ["primary worktree", main],
    ["linked worktree", wtB],
  ]) {
    writeFileSync(join(cwd, "mine.txt"), "declared\n");
    writeFileSync(join(cwd, "foreign.txt"), "not declared\n");
    git(cwd, ["add", "foreign.txt"]); // simulate another agent's staged work
    node(cwd, [scopedCommit, "-m", "scoped", "--", "mine.txt"]);
    const committed = git(cwd, ["show", "--name-only", "--format=", "HEAD"]);
    check(
      `mode-agnostic: scoped-commit stages only declared paths (${label})`,
      committed.includes("mine.txt") && !committed.includes("foreign.txt"),
    );
  }
} finally {
  try {
    execFileSync("git", ["worktree", "remove", "--force", wtB], {
      cwd: main,
      stdio: "ignore",
    });
  } catch {
    /* best-effort */
  }
  rmSync(sandbox, { recursive: true, force: true });
}

console.log(
  failures === 0
    ? "\ncollision-harness: all checks passed"
    : `\ncollision-harness: ${failures} check(s) failed`,
);
process.exit(failures === 0 ? 0 : 1);
