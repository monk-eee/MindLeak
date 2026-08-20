// Tests for update-branch-safely. Run with: node --test scripts/update-branch-safely.test.mjs
//
// The orchestration is exercised through injected collaborators (mirroring
// delivery-queue.mjs's own `nextAction`/`{verifyDirty}` seam) so no test here
// touches a real git remote or gh session.
import assert from "node:assert/strict";
import { test } from "node:test";

import { updateBranchAndVerify } from "./update-branch-safely.mjs";

const io = (overrides = {}) => ({
  prHeadRef: () => "feat/example",
  expectedMergeTree: () => "tree-a",
  updateBranch: () => {},
  actualMergeTree: () => "tree-a",
  ...overrides,
});

test("a clean update reports success and the branch it updated", () => {
  const result = updateBranchAndVerify(42, io());
  assert.equal(result.ok, true);
  assert.equal(result.branch, "feat/example");
  assert.equal(result.message, "updated #42");
});

/// gaps.d/update-branch-can-silently-drop-a-conflicts-losing-side.md: the one
/// case this exists to catch -- a clean `gh pr update-branch` exit whose
/// resulting tree is not the merge it was supposed to produce.
test("a tree mismatch reports the loud warning and fails", () => {
  const result = updateBranchAndVerify(
    42,
    io({ expectedMergeTree: () => "tree-a", actualMergeTree: () => "tree-b" }),
  );
  assert.equal(result.ok, false);
  assert.match(
    result.message,
    /BUT ITS TREE DOES NOT MATCH THE EXPECTED MERGE/,
  );
  assert.match(result.message, /#42/);
  assert.match(result.message, /origin\/feat\/example/);
});

// Sabotage check: the two trees must actually be compared, not merely present.
// Flipping updateBranchAndVerify to ignore the trees entirely would still
// pass a test that only asserted `ok` without checking both directions here.
test("identical trees are never reported as a mismatch, even non-null ones", () => {
  const result = updateBranchAndVerify(
    7,
    io({ expectedMergeTree: () => "same", actualMergeTree: () => "same" }),
  );
  assert.equal(result.ok, true);
});

test("updateBranch runs before the actual tree is read", () => {
  const calls = [];
  updateBranchAndVerify(
    1,
    io({
      updateBranch: () => calls.push("update"),
      actualMergeTree: () => {
        calls.push("actual");
        return "tree-a";
      },
    }),
  );
  assert.deepEqual(calls, ["update", "actual"]);
});
