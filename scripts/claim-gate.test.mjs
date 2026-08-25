// Tests for the ADR-0054 stale-binary diagnostic. Run with: make script-test
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  claimsUnderAnotherIdShape,
  commitBeforeClaimNotice,
  commitsBeforeClaim,
  describeReachabilityFailure,
  missingClaimAdvice,
  parseCallResult,
  publishVerdict,
  reconciliationOf,
  withReconciliationCandidates,
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
    boardReadable: true,
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
    boardReadable: true,
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
    boardReadable: true,
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
    boardReadable: true,
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

// Bug this closes: canonical-push's real board fetch is `include_terminal:
// false` to stay small, so `delivered` (status "done") is never in it -- a
// legitimately-delivered branch could never be recognized as a reconciliation
// and always fell through to "no live Lodestar claim"
// (gaps.d/task-query-board-has-no-response-size-bound.md).
test("without merging in the branch-scoped fetch, a delivered branch is not found (the bug)", () => {
  const nonTerminalOnly = []; // what include_terminal: false actually returns here
  assert.equal(
    reconciliationOf({
      tasks: nonTerminalOnly,
      branch: "feat/already-shipped",
      newCommits: [merge],
    }),
    null,
  );
});

test("merging in the branch-scoped fetch restores reconciliation (the fix)", () => {
  const nonTerminalOnly = [];
  const branchScoped = [delivered]; // task_query(view="board", branch=...)
  const merged = withReconciliationCandidates(nonTerminalOnly, branchScoped);
  const found = reconciliationOf({
    tasks: merged,
    branch: "feat/already-shipped",
    newCommits: [merge],
  });
  assert.equal(found?.id, "task:delivered");
});

test("withReconciliationCandidates keeps the primary fetch's copy of a shared id", () => {
  const primary = [claimed(COLLAPSED)]; // id "task:1", from the general fetch
  const candidates = [{ ...claimed(COLLAPSED), status: "abandoned" }]; // stale duplicate
  const merged = withReconciliationCandidates(primary, candidates);
  assert.equal(merged.length, 1);
  assert.equal(merged[0].status, "claimed");
});

test("withReconciliationCandidates tolerates missing arrays", () => {
  assert.deepEqual(withReconciliationCandidates(undefined, undefined), []);
  assert.deepEqual(withReconciliationCandidates([delivered], undefined), [
    delivered,
  ]);
  assert.deepEqual(withReconciliationCandidates(undefined, [delivered]), [
    delivered,
  ]);
});

// The incident this closes: a stale binary's `board(view="board", branch=...)`
// call answered with a shape `withReconciliationCandidates` could not filter,
// and the bare `catch { reachable = false; }` around the whole batch reported
// the same "unreachable" message a genuinely down server would -- with no way
// to tell them apart short of instrumenting the catch block by hand.
test("describeReachabilityFailure surfaces a real Error's own message", () => {
  assert.equal(
    describeReachabilityFailure(
      new TypeError("(candidates ?? []).filter is not a function"),
    ),
    "(candidates ?? []).filter is not a function",
  );
});

test("describeReachabilityFailure stringifies a non-Error throw", () => {
  assert.equal(describeReachabilityFailure("boom"), "boom");
  assert.equal(describeReachabilityFailure(undefined), "undefined");
});

// The incident this fragment records: a stale binary answered open_session in
// 450ms but could not parse the board, and the gate reported an unreachable
// ledger and offered `cargo build` for a binary that was present. An answered
// ledger whose board cannot be read is a different failure with a different
// remedy, and the refusal must say so.
test("an answered ledger whose board cannot be read is not reported as unreachable", () => {
  const verdict = publishVerdict({
    reachable: true,
    boardReadable: false,
    sessionDeclared: true,
    agent: COLLAPSED,
    tasks: [],
    branch: "fleet/x",
    newCommits: [work],
    now: NOW,
  });

  assert.equal(verdict.ok, false);
  assert.match(verdict.message, /board could not be read/);
  assert.match(verdict.message, /LODESTAR_MCP_BIN/);
  // The wrong remedy last time: a rebuild of a ledger that was answering.
  assert.doesNotMatch(verdict.message, /cargo build/);
  assert.doesNotMatch(verdict.message, /is unreachable/);
});

// A session id was sent but open_session returned no agent. The ledger
// answered, so blaming it as unreachable sends the reader to rebuild the one
// thing that is not broken.
test("an answered ledger that did not identify the session does not read as unreachable", () => {
  const verdict = publishVerdict({
    reachable: true,
    boardReadable: true,
    sessionDeclared: true,
    agent: "",
    tasks: [],
    branch: "fleet/x",
    newCommits: [work],
    now: NOW,
  });

  assert.equal(verdict.ok, false);
  assert.match(verdict.message, /did not identify this session/);
  assert.doesNotMatch(verdict.message, /cargo build/);
  assert.doesNotMatch(verdict.message, /is unreachable/);
});

// gaps.d/commit-then-claim-puts-evidence-before-its-claim.md: a commit's own
// timestamp, not the claim's, decides whether check_conformance can ever
// certify it.
test("a commit before the earliest held claim is flagged", () => {
  const flagged = commitsBeforeClaim(
    [{ sha: "aaa", timestamp: NOW - 10 }],
    [{ claim_started_at: NOW }],
  );
  assert.equal(flagged.length, 1);
  assert.equal(flagged[0].sha, "aaa");
});

test("a commit after the claim it will be certified under is not flagged", () => {
  const flagged = commitsBeforeClaim(
    [{ sha: "aaa", timestamp: NOW + 10 }],
    [{ claim_started_at: NOW }],
  );
  assert.deepEqual(flagged, []);
});

// The whole point of comparing against the EARLIEST claim: a second, later
// claim must never make an already-uncovered commit look flagged, nor may it
// hide one only an earlier claim could have covered.
test("a commit covered by any one held claim is not flagged, even if another started later", () => {
  const flagged = commitsBeforeClaim(
    [{ sha: "aaa", timestamp: NOW - 5 }],
    [{ claim_started_at: NOW - 100 }, { claim_started_at: NOW + 100 }],
  );
  assert.deepEqual(flagged, []);
});

test("no held claims produces no findings", () => {
  assert.deepEqual(
    commitsBeforeClaim([{ sha: "aaa", timestamp: NOW }], []),
    [],
  );
});

test("the notice names every flagged commit and points at the gap fragment", () => {
  const notice = commitBeforeClaimNotice([
    { sha: "aaaaaaaaaaaa", timestamp: NOW - 10 },
    { sha: "bbbbbbbbbbbb", timestamp: NOW - 5 },
  ]);
  assert.match(notice, /2 commit\(s\)/);
  assert.match(notice, /aaaaaaa/);
  assert.match(notice, /bbbbbbb/);
  assert.match(
    notice,
    /gaps\.d\/commit-then-claim-puts-evidence-before-its-claim\.md/,
  );
  assert.match(notice, /This push still succeeds/);
});

test("no flagged commits produces no notice", () => {
  assert.equal(commitBeforeClaimNotice([]), null);
});

// --- parsing a tools/call result ------------------------------------------

/// A tool migrated to the dual Markdown-plus-structuredContent format (e.g.
/// lodestar_stats) must read as the structured object, not the Markdown table
/// string sitting beside it in content[0].text -- reading only content[0].text
/// here silently returned prose instead of data, and every field off it read
/// as undefined.
test("structuredContent is preferred over the Markdown text block", () => {
  const result = {
    content: [{ type: "text", text: "| Count |\n|--:|\n| 3 |" }],
    structuredContent: { open_tasks: 3 },
  };
  assert.deepEqual(parseCallResult(result), { open_tasks: 3 });
});

/// The overwhelmingly common shape for a tool that has not been migrated:
/// its only content is a JSON array or object string.
test("a text block that is valid JSON parses to that value when there is no structuredContent", () => {
  const result = { content: [{ type: "text", text: "[]" }] };
  assert.deepEqual(parseCallResult(result), []);
});

/// A tool answering in genuine prose (no structured form at all) must not
/// throw or vanish -- it is returned as the raw string.
test("a non-JSON text block falls back to the raw string", () => {
  const result = { content: [{ type: "text", text: "no claimable task" }] };
  assert.equal(parseCallResult(result), "no claimable task");
});

test("a result with neither structuredContent nor a text block is undefined", () => {
  assert.equal(parseCallResult({}), undefined);
  assert.equal(parseCallResult({ content: [] }), undefined);
});

/// `null` is how a tool would spell "nothing structured", not "here is null" --
/// it must fall through to the text block exactly like an absent field.
test("a null structuredContent falls through to the text block", () => {
  const result = { content: [{ text: "[1,2]" }], structuredContent: null };
  assert.deepEqual(parseCallResult(result), [1, 2]);
});
