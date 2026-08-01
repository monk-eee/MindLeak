// Tests for the post-push evidence record. Run with: make script-test
//
// The regression these lock down: a publication that records no changed files
// is indistinguishable, to `check_conformance`, from never recording anything
// at all -- both read as "evidence contains no provenance-bearing mutation".
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  memoryPlaneRefusal,
  publicationRecord,
  recordPublication,
} from "./publication-record.mjs";

const SESSION = "c1a8f273b95e4d67a0c214e89f36ab50";

test("an unreachable Memory Plane refuses the publish before it happens", () => {
  // The ordering is the whole defect: the branch used to be on the remote by
  // the time the operator learned the work could never certify.
  const refusal = memoryPlaneRefusal(null);

  assert.match(refusal, /MINDLEAK_MCP_BIN/);
  assert.match(refusal, /cargo build --release/);
  assert.match(refusal, /refuses before pushing/);
});

test("a resolved Memory Plane does not refuse", () => {
  assert.equal(memoryPlaneRefusal("C:/somewhere/mindleak-mcp.exe"), null);
});

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

test("a missing binary names the remedy rather than reporting an outage", () => {
  // The cause that cost two days: a linked worktree has no target/ of its own,
  // so the resolver finds nothing and the old notice called that unreachable.
  const previous = process.env.MINDLEAK_MCP_BIN;
  process.env.MINDLEAK_MCP_BIN = "does-not-exist";
  try {
    const notice = recordPublication({
      repoRoot: process.cwd(),
      sessionId: SESSION,
      sha: "abc",
      message: "x",
    });
    assert.match(notice, /MINDLEAK_MCP_BIN/);
    assert.match(notice, /cargo build --release/);
    assert.match(notice, /no target\/ of its own/);
  } finally {
    if (previous === undefined) delete process.env.MINDLEAK_MCP_BIN;
    else process.env.MINDLEAK_MCP_BIN = previous;
  }
});

test("a bad session id and a missing binary are not the same notice", () => {
  // Reporting one as the other is what turns a one-variable fix into a hunt
  // for an outage that never happened.
  const previous = process.env.MINDLEAK_MCP_BIN;
  process.env.MINDLEAK_MCP_BIN = "does-not-exist";
  try {
    const badSession = recordPublication({
      repoRoot: process.cwd(),
      sessionId: "copilot",
      sha: "abc",
      message: "x",
    });
    const noBinary = recordPublication({
      repoRoot: process.cwd(),
      sessionId: SESSION,
      sha: "abc",
      message: "x",
    });

    assert.notEqual(badSession, noBinary);
    assert.match(badSession, /session id/);
    assert.doesNotMatch(badSession, /MINDLEAK_MCP_BIN/);
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
