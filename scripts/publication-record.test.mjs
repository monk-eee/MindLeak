// Tests for the post-push evidence record. Run with: node --test scripts/
//
// The regression these lock down: a publication that records no changed files
// is indistinguishable, to `check_conformance`, from never recording anything
// at all -- both read as "evidence contains no provenance-bearing mutation".
import assert from "node:assert/strict";
import { test } from "node:test";

import { publicationRecord, recordPublication } from "./publication-record.mjs";

const SESSION = "c1a8f273b95e4d67a0c214e89f36ab50";

test("the record carries the files the push made visible", () => {
  const record = publicationRecord({
    sessionId: SESSION,
    sha: "012f515",
    message: "refactor(memory): split graph/signal",
    changedFiles: ["crates/mindleak-core/src/graph/signal/mod.rs"],
    timestamp: 1_785_217_205,
  });

  assert.deepEqual(record.changed_files, [
    "crates/mindleak-core/src/graph/signal/mod.rs",
  ]);
  assert.equal(record.sha, "012f515");
  assert.equal(record.session_id, SESSION);
  assert.equal(record.timestamp, 1_785_217_205);
});

test("a record with no changed files is still well formed, not undefined", () => {
  // `ingest_commit` defaults this argument away, which would silently produce
  // the empty bundle this whole change exists to stop.
  const record = publicationRecord({
    sessionId: SESSION,
    sha: "abc",
    message: "x",
  });
  assert.deepEqual(record.changed_files, []);
});

test("an unreachable Memory Plane warns instead of failing the push", () => {
  // The commit is already on the remote by this point, so throwing here would
  // trade a missing record for a publisher that reports failure after success.
  const previous = process.env.MINDLEAK_MCP_BIN;
  process.env.MINDLEAK_MCP_BIN = "does-not-exist";
  try {
    const notice = recordPublication({
      repoRoot: process.cwd(),
      sessionId: SESSION,
      sha: "abc",
      message: "x",
      changedFiles: ["a.rs"],
    });
    assert.match(notice, /will not certify/);
  } finally {
    if (previous === undefined) delete process.env.MINDLEAK_MCP_BIN;
    else process.env.MINDLEAK_MCP_BIN = previous;
  }
});

test("a session id that is not a 128-bit token is refused", () => {
  const previous = process.env.MINDLEAK_MCP_BIN;
  delete process.env.MINDLEAK_MCP_BIN;
  try {
    const notice = recordPublication({
      repoRoot: process.cwd(),
      sessionId: "copilot",
      sha: "abc",
      message: "x",
    });
    assert.match(notice, /will not certify/);
  } finally {
    if (previous !== undefined) process.env.MINDLEAK_MCP_BIN = previous;
  }
});
