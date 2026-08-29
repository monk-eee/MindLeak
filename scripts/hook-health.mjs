#!/usr/bin/env node
// Hook-health: the shared Git hooks directory actually holds the hooks the
// fleet relies on.
//
// `.pre-commit-config.yaml` declares
// `default_install_hook_types: [pre-commit, pre-push, post-commit, post-checkout]`,
// but that list only takes effect when `pre-commit install` is re-run. A
// checkout made before a hook type was added keeps working and silently never
// installs the new one — and the hooks directory is shared across every
// worktree, so the drift is fleet-wide, not local. Nothing reported it,
// because the hook that would announce a missing hook is the missing hook:
// `post-commit` records a commit's provenance, so its absence produces an
// empty evidence bundle that looks exactly like an agent who forgot to
// ingest, and the diagnosis lands on the wrong cause (it cost one session two
// wrong theories before anyone checked whether the hook existed).
// `post-checkout` records worktree ownership the moment `git worktree add`
// completes; without it a freshly created worktree reads as unclaimed until
// its first commit.
//
// This runs from pre-push — a path that is itself installed — and refuses the
// push with the one command that reinstalls every declared hook type at once.
//
// Platform-agnostic: git + node only.

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { isOwnedHook } from "./install-hooks.mjs";

// The hooks the fleet depends on, each with the installer that owns it.
// `post-commit` is the one that records evidence; without it work is
// unattributable and nothing says so until completion refuses, hours later
// (ADR-0048). `post-checkout` is the one that records worktree ownership at
// creation (ADR-0038); without it a fresh worktree reads as unclaimed until
// someone commits in it.
//
// The owner matters as much as the presence. `post-checkout` was moved off
// pre-commit deliberately — pre-commit snapshots and restores the whole working
// tree around every hook run, which a checkout has no use for and which was
// measured disturbing trees it was never asked to touch (see
// `scripts/install-hooks.mjs`). A clone made before that move still has
// pre-commit's `post-checkout` shim sitting there, doing the wrong thing while
// reading as installed, so checking only for a file at that path would report
// exactly the state this change exists to remove.
export const EXPECTED_HOOKS = [
  { hook: "pre-commit", owner: "pre-commit" },
  { hook: "pre-push", owner: "pre-commit" },
  { hook: "post-commit", owner: "pre-commit" },
  { hook: "post-checkout", owner: "install-hooks" },
];

// pre-commit writes a generated shim that names itself. A hand-written hook, or
// a stale one left by another tool, does not run pre-commit's configured stages
// — so "installed" means present AND pre-commit's, never merely a file that
// exists at that path.
export function isPreCommitHook(contents) {
  return typeof contents === "string" && contents.includes("pre-commit");
}

const OWNED_BY = {
  "pre-commit": isPreCommitHook,
  "install-hooks": isOwnedHook,
};

// Pure over a reader so both the missing and the complete case are testable
// without a git repository. `readHook(name)` returns the hook file's contents,
// or null when it is absent. Returns `{ hook, owner, present }` so the message
// can distinguish "no hook at all" from "the wrong installer's hook", which
// need different remedies.
export function missingHooks(readHook, expected = EXPECTED_HOOKS) {
  return expected
    .map(({ hook, owner }) => {
      const contents = readHook(hook);
      if (OWNED_BY[owner](contents)) return null;
      return { hook, owner, present: contents !== null };
    })
    .filter(Boolean);
}

// The command that installs each owner's hooks. Both are safe to re-run;
// `--install-hooks` also fetches pre-commit's hook environments, so a checkout
// predating any one hook type ends up fully wired.
export const SETUP_COMMANDS = {
  "pre-commit": "pre-commit install --install-hooks",
  "install-hooks": "node scripts/install-hooks.mjs",
};

export function setupCommand(owner = "pre-commit") {
  return SETUP_COMMANDS[owner];
}

// Named consequences for the two hooks whose absence is otherwise silent: each
// still runs its git-side effect (the commit lands, the worktree is created),
// so nothing else reports what it quietly skipped doing.
const SILENT_CONSEQUENCES = {
  "post-commit":
    "commits land with no provenance and nothing says so until completion " +
    "refuses, hours later (ADR-0048)",
  "post-checkout":
    "a freshly created worktree reads as unclaimed until its first commit " +
    "(ADR-0038)",
};

export function healthMessage(missing) {
  const consequences = missing
    .map(({ hook }) => SILENT_CONSEQUENCES[hook])
    .filter(Boolean);
  const explanation =
    consequences.length > 0
      ? `Without it, ${consequences.join("; and without it, ")}.`
      : "Each missing hook silently stops enforcing what it exists to check.";
  // "Present but the wrong installer's" is called out by name. It is the state
  // a clone made before `post-checkout` moved off pre-commit sits in, and it is
  // strictly more misleading than an absent hook: something runs, so nothing
  // looks broken, while the behaviour being removed carries on.
  const lines = missing.map(
    ({ hook, owner, present }) =>
      `  - ${hook}` +
      (present
        ? ` (present, but not installed by ${owner} — it is another tool's hook` +
          " and does not do what this one must)"
        : " (absent)"),
  );
  const owners = [...new Set(missing.map(({ owner }) => owner))];
  return (
    "hook-health: the shared Git hooks directory is missing hooks the fleet relies on:\n" +
    lines.join("\n") +
    "\n\nHook installation only happens when its installer is re-run, so a\n" +
    "checkout made before a hook changed never gets it — and the hooks\n" +
    `directory is shared across every worktree, so the gap is fleet-wide. ${explanation}\n\n` +
    "Reinstall (safe to re-run):\n" +
    owners.map((owner) => `  ${setupCommand(owner)}`).join("\n")
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
