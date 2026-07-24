// Isolated validate/push helper (ADR-0018). Pushes a commit through the git hooks
// from a throwaway worktree checked out at that commit, so another agent's
// uncommitted or broken WIP in the shared working tree cannot poison your
// pre-push validation. Works identically from the shared tree or a per-agent
// worktree (mode-agnostic).
//
// Platform-agnostic: git + node only. Usage:
//   node scripts/isolated-push.mjs [--commit <ref>] [--remote <name>] [--branch <name>]
// Defaults: --commit HEAD  --remote origin  --branch <current branch>

import { execFileSync } from "node:child_process";
import { existsSync, symlinkSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(name);
  return i !== -1 && args[i + 1] ? args[i + 1] : fallback;
};

const capture = (a, o = {}) =>
  execFileSync("git", a, { encoding: "utf8", ...o }).trim();
const repoRoot = capture(["rev-parse", "--show-toplevel"]);
const gitInherit = (a, o = {}) =>
  execFileSync("git", a, { cwd: repoRoot, stdio: "inherit", ...o });
const gitCapture = (a) => capture(a, { cwd: repoRoot });

const commit = gitCapture(["rev-parse", opt("--commit", "HEAD")]);
const remote = opt("--remote", "origin");
const branch = opt(
  "--branch",
  gitCapture(["rev-parse", "--abbrev-ref", "HEAD"]),
);

const worktree = join(tmpdir(), `mindleak-isolated-push-${process.pid}`);
let code = 0;
try {
  gitInherit(["worktree", "add", "--detach", "--quiet", worktree, commit]);
  // Link gitignored tool deps so npm-based hooks (eslint/prettier) work in the
  // throwaway worktree, which only checks out tracked files.
  for (const dir of ["editors/vscode/node_modules"]) {
    const src = join(repoRoot, dir);
    const dst = join(worktree, dir);
    if (existsSync(src) && !existsSync(dst)) {
      try {
        symlinkSync(src, dst, "junction");
      } catch {
        /* fall through — the npm hook reports if deps are missing */
      }
    }
  }
  execFileSync("git", ["push", remote, `HEAD:refs/heads/${branch}`], {
    cwd: worktree,
    stdio: "inherit",
  });
  console.log(
    `isolated-push: pushed ${commit.slice(0, 10)} -> ${remote}/${branch}`,
  );
} catch (err) {
  code = typeof err.status === "number" ? err.status : 1;
} finally {
  try {
    gitInherit(["worktree", "remove", "--force", worktree]);
  } catch {
    /* best-effort; `git worktree prune` reclaims a stale entry */
  }
}
process.exit(code);
