// Tests for the auto-merge guard. Run with: make script-test
//
// The module exists so this decision can be tested without a network, a token,
// or a real `gh` — and until now it had no tests at all.
import { test } from "node:test";
import assert from "node:assert/strict";

import {
  armedPullRequestNumber,
  armedRefusal,
  publishPromisedBranch,
  rearmFailure,
} from "./auto-merge-guard.mjs";

const armed = (number) =>
  JSON.stringify({
    number,
    state: "OPEN",
    autoMergeRequest: { enabledAt: "now" },
  });

test("an armed open pull request is reported by number", () => {
  assert.equal(armedPullRequestNumber(armed(134)), 134);
});

test("an unarmed pull request promises nothing", () => {
  assert.equal(
    armedPullRequestNumber(
      JSON.stringify({ number: 1, state: "OPEN", autoMergeRequest: null }),
    ),
    null,
  );
});

test("a closed pull request cannot merge anything", () => {
  assert.equal(
    armedPullRequestNumber(
      JSON.stringify({ number: 1, state: "CLOSED", autoMergeRequest: {} }),
    ),
    null,
  );
});

// A guard that blocks on its own blindness is unsatisfiable: there is no way
// for the caller to make `gh` available from inside the failing push.
test("an unavailable or unparseable answer does not invent one", () => {
  assert.equal(armedPullRequestNumber(null), null);
  assert.equal(armedPullRequestNumber(""), null);
  assert.equal(armedPullRequestNumber("not json"), null);
});

// ---- publishing to a branch whose merge is promised away --------------------

const recorder = () => {
  const calls = [];
  return {
    calls,
    disarm: (n) => calls.push(`disarm:${n}`),
    push: () => calls.push("push"),
    rearm: (n) => calls.push(`rearm:${n}`),
  };
};

test("an unpromised branch is pushed without touching auto-merge", () => {
  const r = recorder();
  const result = publishPromisedBranch({ number: null, ...r });
  assert.deepEqual(r.calls, ["push"]);
  assert.equal(result.cycled, false);
});

// The ordering IS the safety property: at no point may an armed promise exist
// about a branch being written to. Asserting the sequence is the only way to
// see that, and it is exactly what a live smoke test could not observe.
test("the promise is withdrawn before the write and re-made after it", () => {
  const r = recorder();
  const result = publishPromisedBranch({ number: 134, ...r });
  assert.deepEqual(r.calls, ["disarm:134", "push", "rearm:134"]);
  assert.equal(result.cycled, true);
  assert.equal(result.rearmed, true);
});

// A failed push leaves the branch exactly as the promise already described, so
// the promise is restored rather than dropped on the floor.
test("a failed push still restores the promise, then reports the failure", () => {
  const calls = [];
  assert.throws(
    () =>
      publishPromisedBranch({
        number: 7,
        disarm: (n) => calls.push(`disarm:${n}`),
        push: () => {
          calls.push("push");
          throw new Error("remote rejected");
        },
        rearm: (n) => calls.push(`rearm:${n}`),
      }),
    /remote rejected/,
  );
  assert.deepEqual(calls, ["disarm:7", "push", "rearm:7"]);
});

// Left disarmed is the safe direction: work sits unmerged and visible, rather
// than merging something nobody promised. But it must be said out loud.
test("a re-arm that fails is reported rather than swallowed", () => {
  const result = publishPromisedBranch({
    number: 42,
    disarm: () => {},
    push: () => {},
    rearm: () => {
      throw new Error("gh exploded");
    },
  });
  assert.equal(result.pushed, true);
  assert.equal(result.rearmed, false);
  assert.match(result.rearmError.message, /gh exploded/);
  assert.match(rearmFailure(42), /gh pr merge 42 --merge --auto/);
});

test("the refusal message names the pull request and the branch", () => {
  const message = armedRefusal(134, "fix/thing");
  assert.match(message, /#134/);
  assert.match(message, /fix\/thing/);
});
