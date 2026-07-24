// Scoped, isolation-aware cargo runner for MindLeak's git hooks
// (see .pre-commit-config.yaml). Stops concurrent-agent "validation poisoning"
// when several agents share one working tree (ADR-0018), two ways:
//
//   1. SCOPE — run cargo only for the crate packages this change touches, so an
//      unrelated agent's broken crate cannot fail your commit/push. This alone
//      removes the common cross-crate poisoning (a `mindleak-core` commit no
//      longer compiles `lodestar-mcp` at all).
//   2. SNAPSHOT — when the live working tree could leak another agent's files
//      into the build, materialize the staged tree (commit stage) or HEAD tree
//      (push stage) through a temporary Git index. This validates exact bytes
//      without creating a worktree, branch, commit, or shared ref.
//
// Platform-agnostic: git + cargo + node only. Usage:
//   node scripts/cargo-precommit.mjs <fmt|clippy|test> <commit|push>

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

const mode = process.argv[2];
const stage = process.argv[3];
if (
  !["fmt", "clippy", "test"].includes(mode) ||
  !["commit", "push"].includes(stage)
) {
  console.error("usage: cargo-precommit.mjs <fmt|clippy|test> <commit|push>");
  process.exit(2);
}

const exitOf = (err) => (typeof err.status === "number" ? err.status : 1);
const gitCapture = (args, root) =>
  execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();

const repoRoot = execFileSync("git", ["rev-parse", "--show-toplevel"], {
  encoding: "utf8",
}).trim();
const git = (args) => gitCapture(args, repoRoot);

// ---- 1. affected crate packages ---------------------------------------------
function changedFiles() {
  if (stage === "commit") {
    return git(["diff", "--cached", "--name-only", "--diff-filter=ACMR"])
      .split("\n")
      .map((s) => s.trim())
      .filter(Boolean);
  }
  // push: diff the commits being pushed against their upstream.
  let base = "";
  for (const ref of ["@{push}", "@{upstream}", "origin/main"]) {
    try {
      base = git(["rev-parse", "--verify", "--quiet", ref]);
      if (base) break;
    } catch {
      base = "";
    }
  }
  if (!base) return null; // no resolvable base — validate everything
  try {
    return git(["diff", "--name-only", "--diff-filter=ACMR", `${base}...HEAD`])
      .split("\n")
      .map((s) => s.trim())
      .filter(Boolean);
  } catch {
    return null;
  }
}

function packageName(crateDir) {
  const cargo = `crates/${crateDir}/Cargo.toml`;
  let source;
  try {
    source =
      stage === "commit"
        ? git(["show", `:${cargo}`])
        : git(["show", `HEAD:${cargo}`]);
  } catch {
    return null;
  }
  const match = source.match(/^\s*name\s*=\s*"([^"]+)"/m);
  return match ? match[1] : null;
}

const files = changedFiles();
let allCrates = false;
let crateDirs = [];
let pkgs = [];
if (files === null) {
  allCrates = true;
} else {
  const dirs = new Set();
  for (const file of files) {
    const match = file.replace(/\\/g, "/").match(/^crates\/([^/]+)\//);
    if (match) dirs.add(match[1]);
  }
  crateDirs = [...dirs];
  pkgs = [...new Set(crateDirs.map(packageName).filter(Boolean))];
  if (pkgs.length === 0) {
    process.exit(0); // no Rust crate touched — nothing for cargo to do
  }
}

// ---- 2. decide whether to isolate -------------------------------------------
// Commit stage: pre-commit already stashes unstaged TRACKED changes, so the only
// residual leak is UNTRACKED files inside an affected crate (e.g. a concurrent
// file -> dir split). Isolate only then. Push stage: the working tree carries
// every agent's uncommitted WIP, so always validate the pushed commit itself.
function foreignUntrackedInAffected() {
  if (allCrates || crateDirs.length === 0) return false;
  const paths = crateDirs.map((dir) => `crates/${dir}/`);
  const out = git([
    "ls-files",
    "--others",
    "--exclude-standard",
    "--",
    ...paths,
  ]);
  return out.trim().length > 0;
}

const isolate = stage === "push" || foreignUntrackedInAffected();

const scope = allCrates
  ? [mode === "fmt" ? "--all" : "--workspace"]
  : pkgs.flatMap((pkg) => ["-p", pkg]);

let args;
if (mode === "fmt") {
  // Auto-format in place on the fast path; check-only when isolated (a throwaway
  // snapshot cannot fix the developer's files).
  args = isolate ? ["fmt", ...scope, "--", "--check"] : ["fmt", ...scope, "--"];
} else if (mode === "clippy") {
  args = [
    "clippy",
    ...scope,
    "--all-targets",
    "--all-features",
    "--",
    "-D",
    "warnings",
  ];
} else {
  args = ["test", ...scope];
}

// ---- run ---------------------------------------------------------------------
if (!isolate) {
  try {
    execFileSync("cargo", args, { cwd: repoRoot, stdio: "inherit" });
    process.exit(0);
  } catch (err) {
    process.exit(exitOf(err));
  }
}

const snapshot =
  stage === "commit" ? git(["write-tree"]) : git(["rev-parse", "HEAD^{tree}"]);
const snapshotRoot = mkdtempSync(
  join(tmpdir(), `mindleak-hook-${mode}-${process.pid}-`),
);
const snapshotDir = join(snapshotRoot, "files");
const snapshotIndex = join(snapshotRoot, "index");
const targetDir = join(repoRoot, "target", "hooks");
mkdirSync(snapshotDir, { recursive: true });
mkdirSync(targetDir, { recursive: true });

let code = 0;
try {
  const snapshotEnv = { ...process.env, GIT_INDEX_FILE: snapshotIndex };
  execFileSync("git", ["read-tree", snapshot], {
    cwd: repoRoot,
    stdio: "inherit",
    env: snapshotEnv,
  });
  const prefix = `${snapshotDir.replace(/\\/g, "/")}/`;
  execFileSync(
    "git",
    ["checkout-index", "--all", "--force", `--prefix=${prefix}`],
    { cwd: repoRoot, stdio: "inherit", env: snapshotEnv },
  );
  execFileSync("cargo", args, {
    cwd: snapshotDir,
    stdio: "inherit",
    env: { ...process.env, CARGO_TARGET_DIR: targetDir },
  });
} catch (err) {
  code = exitOf(err);
} finally {
  rmSync(snapshotRoot, { recursive: true, force: true });
}
process.exit(code);
