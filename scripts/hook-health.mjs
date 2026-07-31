#!/usr/bin/env node
// Hook-health: the shared Git hooks directory actually holds the hooks the
// fleet relies on.
//
// `.pre-commit-config.yaml` declares
// `default_install_hook_types: [pre-commit, pre-push, post-commit]`, but that
// list only takes effect when `pre-commit install` is re-run. A checkout made
// before a hook type was added keeps working and silently never installs the
// new one — and the hooks directory is shared across every worktree, so the
// drift is fleet-wide, not local. Nothing reported it, because the hook that
// would announce a missing hook is the missing hook: `post-commit` records a
// commit's provenance, so its absence produces an empty evidence bundle that
// looks exactly like an agent who forgot to ingest, and the diagnosis lands on
// the wrong cause (it cost one session two wrong theories before anyone checked
// whether the hook existed).
//
// This runs from pre-push — a path that is itself installed — and refuses the
// push with the one command that reinstalls every declared hook type at once.
//
// Platform-agnostic: git + node only.

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

// The hooks pre-commit installs and the fleet depends on. `post-commit` is the
// one that records evidence; without it work is unattributable and nothing says
// so until completion refuses, hours later (ADR-0048).
export const EXPECTED_HOOKS = ["pre-commit", "pre-push", "post-commit"];

// pre-commit writes a generated shim that names itself. A hand-written hook, or
// a stale one left by another tool, does not run pre-commit's configured stages
// — so "installed" means present AND pre-commit's, never merely a file that
// exists at that path.
export function isPreCommitHook(contents) {
  return typeof contents === "string" && contents.includes("pre-commit");
}

// Pure over a reader so both the missing and the complete case are testable
// without a git repository. `readHook(name)` returns the hook file's contents,
// or null when it is absent.
export function missingHooks(readHook, expected = EXPECTED_HOOKS) {
  return expected.filter((hook) => !isPreCommitHook(readHook(hook)));
}

// The single command that installs every declared hook type. `--install-hooks`
// also fetches the hook environments, so it is safe to re-run and leaves a
// checkout that predates any one hook type fully wired.
export function setupCommand() {
  return "pre-commit install --install-hooks";
}

export function healthMessage(missing) {
  return (
    "hook-health: the shared Git hooks directory is missing hooks the fleet relies on:\n" +
    missing.map((hook) => `  - ${hook}`).join("\n") +
    "\n\n`default_install_hook_types` only installs on `pre-commit install`, so a\n" +
    "checkout made before a hook type was added never gets it — and the hooks\n" +
    "directory is shared across every worktree, so the gap is fleet-wide. Without\n" +
    "post-commit, commits land with no provenance and nothing says so until\n" +
    "completion refuses, hours later (ADR-0048).\n\n" +
    "Reinstall every declared hook type (safe to re-run):\n" +
    `  ${setupCommand()}`
  );
}

// Read hooks from a resolved directory. Absent or unreadable → null, which the
// logic treats as "not installed".
export function readHookFrom(hooksDir) {
  return (hook) => {
    try {
      return readFileSync(join(hooksDir, hook), "utf8");
    } catch {
      return null;
    }
  };
}

// Resolve the hooks directory git will actually use: honours core.hooksPath
// and, in a linked worktree, the shared common hooks dir (`hooks` is a common
// path, so every worktree resolves to the same one).
export function resolveHooksDir(repoRoot) {
  const raw = execFileSync("git", ["rev-parse", "--git-path", "hooks"], {
    cwd: repoRoot,
    encoding: "utf8",
  }).trim();
  return resolve(repoRoot, raw);
}

function main() {
  let repoRoot;
  try {
    repoRoot = execFileSync("git", ["rev-parse", "--show-toplevel"], {
      encoding: "utf8",
    }).trim();
  } catch {
    // No git repository — nothing to verify, and this must never block a
    // caller that is not in one.
    return;
  }
  const missing = missingHooks(readHookFrom(resolveHooksDir(repoRoot)));
  if (missing.length > 0) {
    console.error(healthMessage(missing));
    process.exit(1);
  }
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
