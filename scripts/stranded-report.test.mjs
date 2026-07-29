// Tests for the stranded-claim report. Run with: node --test scripts/
//
// The report proposes a commit for a human to confirm. Its whole value is that
// a weak guess reads as a weak guess -- a confident-looking wrong answer is
// worse than no answer, because it converts a judgement into a rubber stamp.
import assert from "node:assert/strict";
import { test } from "node:test";

import { bestMatch, confidence, similarity } from "./stranded-report.mjs";

const commit = (sha, subject) => ({ sha, at: 0, subject });

test("a commit that restates the task scores highly", () => {
  const score = similarity(
    "Block any merge driver returning to .gitattributes",
    "fix(delivery): block any merge driver returning to .gitattributes",
  );
  assert.ok(score > 0.9, `expected a near-exact match, got ${score}`);
});

/// Every title in this repository contains "the", "must" and "a". If those
/// counted, everything would match everything and the report would be noise
/// wearing the shape of an answer.
test("common words carry no signal", () => {
  const score = similarity(
    "The board must report what it cannot close",
    "docs: a note about the thing that must not be a thing",
  );
  assert.ok(score < 0.3, `expected near-zero, got ${score}`);
});

test("the best match wins and the runner-up is reported", () => {
  const { best, next } = bestMatch(
    "Split graph/query into cohesive submodules",
    [
      commit(
        "aaa",
        "refactor(memory): split graph/query into cohesive submodules",
      ),
      commit(
        "bbb",
        "refactor(core): split store/design into cohesive submodules",
      ),
    ],
  );
  assert.equal(best.sha, "aaa");
  assert.equal(next.sha, "bbb");
});

/// The distinction that matters. Two near-identical refactor commits differ by
/// one word, so picking one is a coin toss dressed as a finding -- it must not
/// be presented as strong.
test("a close second downgrades confidence", () => {
  const clear = confidence({ score: 0.9 }, { score: 0.2 });
  assert.equal(clear, "strong");

  const contested = confidence({ score: 0.9 }, { score: 0.85 });
  assert.notEqual(contested, "strong");
});

test("nothing resembling the task is reported as no match, not a bad one", () => {
  const { best, next } = bestMatch(
    "Pilot with independent multi-agent developers",
    [commit("aaa", "chore: bump a dependency")],
  );
  assert.equal(confidence(best, next), "none");
});

/// A task whose title is only stopwords must not silently score 1.0 against
/// everything by dividing by zero.
test("a title with no distinctive words matches nothing", () => {
  assert.equal(similarity("the and or of", "fix: something real"), 0);
});
