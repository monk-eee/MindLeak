// A path for `git worktree add` to create, reserved without ever being
// created and deleted first (gaps.d/a-scoped-commit-fixture-can-fall-outside-
// its-repository.md). `mkdtempSync` unavoidably creates the directory it
// names, so using its result directly as a worktree target meant deleting
// that directory so `git worktree add` could recreate it at the same path --
// a real create/delete/recreate race against anything else touching that
// exact path in between, observed intermittently on Windows as a fixture's
// own git commands running outside any repository at all.
//
// Reserving a real, always-existing PARENT directory and returning a
// not-yet-created child path inside it removes the race by construction:
// nothing is ever deleted, and the child name has never existed for
// anything else to collide with.
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/**
 * @param {string} prefix passed through to mkdtempSync for the parent directory.
 * @param {{ mkdtemp?: typeof mkdtempSync }} [deps]
 * @returns {{ parent: string, path: string }} `parent` already exists; `path`
 *   (a `worktree` child of it) does not, and is safe to pass to
 *   `git worktree add` directly.
 */
export function reserveWorktreePath(prefix, { mkdtemp = mkdtempSync } = {}) {
  const parent = mkdtemp(join(tmpdir(), prefix));
  return { parent, path: join(parent, "worktree") };
}
