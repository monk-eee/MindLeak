#!/usr/bin/env node
// A manual, single-PR counterpart to delivery-queue.mjs's own update-branch
// tree-mismatch guard (ADR-0062).
//
// gaps.d/update-branch-can-silently-drop-a-conflicts-losing-side.md: the
// queue's own guarded call can no longer repeat PR #507's failure unnoticed,
// but "a manual `gh pr update-branch` run outside the queue... is not
// covered by this guard." This is that coverage, reusing the exact same
// verified predicate and tree readers rather than reimplementing them.
//
// Usage:
//   node scripts/update-branch-safely.mjs <pr-number>

import { execFileSync } from "node:child_process";
import process from "node:process";
import {
  expectedMergeTree,
  actualMergeTree,
  updateBranchMismatch,
} from "./delivery-queue.mjs";

function gh(args) {
  return execFileSync("gh", args, {
    encoding: "utf8",
    stdio: ["pipe", "pipe", "pipe"],
  }).trim();
}

function prHeadRef(number) {
  const json = gh(["pr", "view", String(number), "--json", "headRefName"]);
  return JSON.parse(json).headRefName;
}

/**
 * Update one PR's branch and report whether the resulting tree matches the
 * merge it was supposed to produce.
 *
 * Pure over its injected collaborators, so this is unit-testable without a
 * real git remote or gh session -- the same DI seam this file's own
 * `nextAction`/`{verifyDirty}` already uses. The real git/gh calls
 * (`expectedMergeTree`/`actualMergeTree`/`updateBranch`) are the queue's own
 * proven functions, not a second implementation to drift from them.
 */
export function updateBranchAndVerify(number, io) {
  const branch = io.prHeadRef(number);
  const expectedTree = io.expectedMergeTree(branch);
  io.updateBranch(number);
  const actualTree = io.actualMergeTree(branch);
  if (updateBranchMismatch(expectedTree, actualTree)) {
    return {
      ok: false,
      branch,
      message:
        `updated #${number} BUT ITS TREE DOES NOT MATCH THE EXPECTED MERGE -- ` +
        `gh pr update-branch may have silently dropped a side. Reconcile ` +
        `#${number} by hand: fetch and diff origin/${branch} against a fresh ` +
        "local `git merge origin/main` before trusting it.",
    };
  }
  return { ok: true, branch, message: `updated #${number}` };
}

const USAGE = `update-branch-safely -- gh pr update-branch, with the same tree-mismatch
guard delivery-queue.mjs already uses for its own queued updates (ADR-0062,
gaps.d/update-branch-can-silently-drop-a-conflicts-losing-side.md).

  node scripts/update-branch-safely.mjs <pr-number>

Use this instead of a bare \`gh pr update-branch <n>\` any time a branch is
being reconciled by hand, outside the delivery queue.`;

function main() {
  const arg = process.argv[2];
  if (!arg || !/^\d+$/.test(arg)) {
    console.log(USAGE);
    process.exitCode = arg ? 1 : 0;
    return;
  }
  const number = Number(arg);
  const result = updateBranchAndVerify(number, {
    prHeadRef,
    expectedMergeTree,
    actualMergeTree,
    updateBranch: (n) => gh(["pr", "update-branch", String(n)]),
  });
  console.log(result.message);
  process.exitCode = result.ok ? 0 : 1;
}

if (
  process.argv[1] &&
  import.meta.url.endsWith(process.argv[1].replace(/\\/g, "/"))
) {
  main();
}
