import assert from "node:assert/strict";
import { test } from "node:test";

import { removeTreeSafely } from "./fs-retry.mjs";

test("removeTreeSafely reports success without exposing rmSync's return value", () => {
  const calls = [];
  const result = removeTreeSafely("/some/dir", {
    rm: (path) => calls.push(path),
  });
  assert.deepEqual(result, { ok: true });
  assert.deepEqual(calls, ["/some/dir"]);
});

test("removeTreeSafely asks rmSync to retry a transient lock itself", () => {
  // Regression: neither call site this replaced ever passed maxRetries, so a
  // directory a build had only just released (Windows EPERM/EBUSY on a
  // pending-delete file) failed on the very first attempt instead of
  // clearing the way a rebuilt cache normally does moments later.
  let seenOptions = null;
  removeTreeSafely("/some/dir", {
    rm: (_path, options) => {
      seenOptions = options;
    },
  });
  assert.equal(seenOptions.recursive, true);
  assert.equal(seenOptions.force, true);
  assert.ok(seenOptions.maxRetries > 0, "must ask Node to retry at all");
  assert.ok(seenOptions.retryDelay > 0, "must wait between retries");
});

test("removeTreeSafely reports a failure that outlasts every retry instead of throwing", () => {
  // Regression: scripts/worktree-reclaim.mjs and scripts/artefact-sweep.mjs
  // each called a bare rmSync in a loop over independent directories; one
  // still-locked path crashed the whole run with an uncaught EPERM instead of
  // leaving that one directory for a later pass and continuing with the rest.
  const error = Object.assign(new Error("permission denied"), {
    code: "EPERM",
  });
  const result = removeTreeSafely("/locked/dir", {
    rm: () => {
      throw error;
    },
  });
  assert.deepEqual(result, { ok: false, error });
});
