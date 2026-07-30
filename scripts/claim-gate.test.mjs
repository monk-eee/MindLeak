// Tests for the ADR-0054 stale-binary diagnostic. Run with: make script-test
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  claimsUnderAnotherIdShape,
  missingClaimAdvice,
  publishVerdict,
  reconciliationOf,
} from "./claim-gate.mjs";

const NOW = 1_000_000;
const FINGERPRINT = "bff9bbe3968f16636cbc5522086114e3";
const COLLAPSED = `session:v1:${FINGERPRINT}`;
const LABELLED = `session:v1:copilot:${FINGERPRINT}`;

const claimed = (owner) => ({
  id: "task:1",
  status: "claimed",
  owner,
  lease_expires_at: NOW + 600,
});

/// The incident this exists for: a server binary built before ADR-0054 resolves
/// the labelled id, the migrated ledger holds the collapsed one, and every
/// downstream guard is then correct and useless. The gate said "no live claim",
/// `claim_task` returned `won: false` on a task the session already owned, and
/// the overlap notice blamed a peer for the caller's own work — three different
/// lies from one stale binary, none of which named it.
test("a claim held under the pre-ADR-0054 id shape is identified, not silently missed", () => {
  const shifted = claimsUnderAnotherIdShape(
    [claimed(COLLAPSED)],
    LABELLED,
    NOW,
  );
  assert.equal(shifted.length, 1);
  assert.equal(shifted[0].owner, COLLAPSED);
});

test("the advice names the stale binary and says re-claiming will not help", () => {
  const advice = missingClaimAdvice([claimed(COLLAPSED)], LABELLED, NOW);
  assert.match(advice, /ADR-0054/);
  assert.match(advice, /Rebuild and reinstall/);
  assert.match(advice, /re-claiming will not help/);
  assert.match(advice, /the claim you already hold is intact/);
});

/// Guessing at a stale binary every time a claim is missing would teach readers
/// to skip the line, which is how a diagnostic becomes noise.
test("an ordinary missing claim does not blame the binary", () => {
  const advice = missingClaimAdvice([], COLLAPSED, NOW);
  assert.equal(advice, "claim a task before publishing.");
  assert.doesNotMatch(advice, /ADR-0054/);
});

/// A different session that merely happens to be unclaimed must never be read
/// as this one. The fingerprint is the whole basis of the match.
test("another session's claim is not mistaken for this one under any id shape", () => {
  const other = "session:v1:0123456789abcdef0123456789abcdef";
  assert.equal(
    claimsUnderAnotherIdShape([claimed(other)], LABELLED, NOW).length,
    0,
  );
});

/// The diagnostic explains a refusal; it must never become one.
test("the shifted claim still refuses the publication", () => {
  const verdict = publishVerdict({
    reachable: true,
    sessionDeclared: true,
    agent: LABELLED,
    branch: "fleet/x",
    tasks: [claimed(COLLAPSED)],
    now: NOW,
  });
  assert.equal(
    verdict.ok,
    false,
    "a mismatched identity is not a licence to publish",
  );
  assert.match(verdict.message, /ADR-0054/);
});

// A delivered branch must stay reconcilable. Completing a task releases its
// claim, so `main` moving left the pull request permanently stale: the queue
// stepped over it, and each rescue invented a throwaway task to get past the
// gate. #168 needed that three times. Minting a task per republish is how six
// duplicate tasks reached the board.
const delivered = {
  id: "task:delivered",
  status: "done",
  branch: "feat/already-shipped",
  owner: COLLAPSED,
};
const merge = { sha: "aaa", isMerge: true };
const work = { sha: "bbb", isMerge: false };

test("a finished task's branch may be reconciled without inventing a new task", () => {
  const verdict = publishVerdict({
    reachable: true,
    sessionDeclared: true,
    agent: COLLAPSED,
    tasks: [delivered],
    branch: "feat/already-shipped",
    newCommits: [merge],
    now: NOW,
  });

  assert.equal(verdict.ok, true);
  assert.equal(verdict.reconciles.id, "task:delivered");
  assert.match(verdict.notice, /attributed to that task/);
});

// The narrow part. Without this the exemption reads "finish a task, then push
// anything to that branch forever", which is a bypass wearing a fix's clothes.
test("real work on a delivered branch still requires a claim", () => {
  const verdict = publishVerdict({
    reachable: true,
    sessionDeclared: true,
    agent: COLLAPSED,
    tasks: [delivered],
    branch: "feat/already-shipped",
    newCommits: [merge, work],
    now: NOW,
  });

  assert.equal(verdict.ok, false);
  assert.match(verdict.message, /no live Lodestar claim/);
});

/// The remediation an agent reads at the moment it is blocked, and therefore
/// the moment it is most likely to copy an instruction verbatim.
///
/// It named `claim_task` and `create_task`, both retired by ADR-0059. Nothing
/// was broken — the deprecation table still answers them — so no test, type or
/// lint could notice; advice is a string. What it cost was a reader taught the
/// retiring name by the tool that stopped them, and one more caller to migrate
/// before the removal train. The verbs are asserted here rather than the whole
/// sentence so the wording stays free to improve.
test("the remediation names verbs the server still advertises", () => {
  const verdict = publishVerdict({
    reachable: true,
    sessionDeclared: true,
    agent: COLLAPSED,
    tasks: [],
    branch: "feat/unclaimed",
    newCommits: [work],
    now: NOW,
  });

  assert.equal(verdict.ok, false);
  assert.match(verdict.message, /task_claim\(task_id, step="claim"\)/);
  assert.match(verdict.message, /task_create\(goal_id, title, acceptance\)/);
  for (const retired of ["claim_task(", "create_task("]) {
    assert.equal(
      verdict.message.includes(retired),
      false,
      `the advice still teaches ${retired}, which ADR-0059 retired`,
    );
  }
});

test("an unrelated branch is not reconciliation, however finished the task is", () => {
  assert.equal(
    reconciliationOf({
      tasks: [delivered],
      branch: "feat/something-else",
      newCommits: [merge],
    }),
    null,
  );
});

// A branch nobody ever claimed has nothing to attribute a push to, so the
// absence of new commits must not read as "reconciliation".
test("no new commits is not a reconciliation", () => {
  assert.equal(
    reconciliationOf({
      tasks: [delivered],
      branch: "feat/already-shipped",
      newCommits: [],
    }),
    null,
  );
});

test("a still-open task's branch is not reconcilable: claim it instead", () => {
  assert.equal(
    reconciliationOf({
      tasks: [{ ...delivered, status: "open" }],
      branch: "feat/already-shipped",
      newCommits: [merge],
    }),
    null,
  );
});
