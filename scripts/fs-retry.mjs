// Windows can transiently refuse to remove a directory whose handle a
// process only just released: NTFS marks a file for pending delete before
// the directory entry actually clears, so a build or antivirus scan that let
// go of a handle microseconds ago can still make a clean rmSync fail with
// EPERM/EBUSY -- the same class of race
// crates/mindleak-storage/src/repository/migrate.rs already retries around
// for its own migration lock. Node's own rmSync already retries exactly this
// (maxRetries/retryDelay); the gap at every call site this replaced was never
// passing them, and none caught the case retrying still does not clear -- one
// locked directory should not abort every other worktree or cache queued
// behind it in the same run.
//
// Platform-agnostic: node only.

import { rmSync } from "node:fs";

/** Remove a directory tree, retrying a transient lock; reports rather than throws. */
export function removeTreeSafely(path, { rm = rmSync } = {}) {
  try {
    rm(path, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
    return { ok: true };
  } catch (error) {
    return { ok: false, error };
  }
}
