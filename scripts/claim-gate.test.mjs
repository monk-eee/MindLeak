// Tests for the ADR-0054 stale-binary diagnostic. Run with: node --test scripts/
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  claimsUnderAnotherIdShape,
  missingClaimAdvice,
  publishVerdict,
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
