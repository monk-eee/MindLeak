// Tests for the post-push evidence record. Run with: make script-test
//
// The regression these lock down: a publication that records no changed files
// is indistinguishable, to `check_conformance`, from never recording anything
// at all -- both read as "evidence contains no provenance-bearing mutation".
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
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

test("the record carries the supplied commit facts", () => {
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

// Publication used to reattribute the whole branch diff to its newest commit.
// Only that commit's own paths and timestamp may enter mutation provenance.
test("publication records the exact commit rather than caller-supplied branch facts", () => {
  const sha = "b".repeat(40);
  const calls = [];
  const notice = recordPublication(
    {
      repoRoot: "/publication-fixture",
      sessionId: SESSION,
      sha,
      message: "untrusted branch description",
      changedFiles: ["earlier.rs", "latest.rs"],
      timestamp: 2000,
    },
    {
      resolve: () => "fixture-mindleak",
      run: (args) => {
        assert.equal(
          args.at(-1),
          sha,
          "Git must read the published commit, not a later HEAD",
        );
        if (args[0] === "log")
          return `${sha}\u00001000\u0000fix: actual\n\nWHY: actual rationale`;
        if (args[0] === "show") return "latest.rs\n";
        assert.fail(`unexpected git ${args}`);
      },
      call: (_server, _root, requests) => calls.push(...requests),
    },
  );
  assert.equal(notice, null);
  assert.deepEqual(
    calls.find((request) => request.name === "ingest_commit")?.arguments,
    {
      session_id: SESSION,
      sha,
      message: "fix: actual\n\nWHY: actual rationale",
      changed_files: ["latest.rs"],
      timestamp: 1000,
    },
  );
});

test("publication does not invent evidence when Git cannot read the commit", () => {
  const notice = recordPublication(
    {
      repoRoot: "/publication-fixture",
      sessionId: SESSION,
      sha: "c".repeat(40),
    },
    {
      resolve: () => "fixture-mindleak",
      run: () => {
        throw new Error("missing commit");
      },
      call: () => assert.fail("unresolved commit facts must never be ingested"),
    },
  );
  assert.match(notice, /Git could not read the published commit/);
});

test("publishing one commit never attributes earlier branch work or a later HEAD to it", () => {
  const repoRoot = mkdtempSync(join(tmpdir(), "mindleak-publication-"));
  const env = { ...process.env };
  for (const variable of Object.keys(env)) {
    if (variable.startsWith("GIT_")) delete env[variable];
  }
  const run = (args) =>
    execFileSync("git", args, {
      cwd: repoRoot,
      encoding: "utf8",
      stdio: "pipe",
      env,
    }).trim();
  const calls = [];
  try {
    run(["init", "-b", "main"]);
    run(["config", "user.name", "Publication Test"]);
    run(["config", "user.email", "publication@example.invalid"]);
    run(["config", "core.hooksPath", join(repoRoot, "no-hooks")]);
    for (const name of ["earlier", "published", "later"]) {
      writeFileSync(join(repoRoot, `${name}.rs`), `${name}\n`);
      run(["add", `${name}.rs`]);
      run([
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-m",
        `fix: ${name}`,
        "-m",
        `WHY: ${name} only`,
      ]);
    }
    const sha = run(["rev-parse", "HEAD~1"]);
    const timestamp = Number(run(["log", "-1", "--format=%ct", sha]));
    const notice = recordPublication(
      {
        repoRoot,
        sessionId: SESSION,
        sha,
        changedFiles: ["earlier.rs", "published.rs", "later.rs"],
        timestamp: timestamp + 86400,
      },
      {
        resolve: () => "fixture-mindleak",
        run,
        call: (_server, _root, requests) => calls.push(...requests),
      },
    );
    assert.equal(notice, null);
    assert.deepEqual(
      calls.find((request) => request.name === "ingest_commit")?.arguments,
      {
        session_id: SESSION,
        sha,
        timestamp,
        message: "fix: published\n\nWHY: published only",
        changed_files: ["published.rs"],
      },
    );
  } finally {
    rmSync(repoRoot, { recursive: true, force: true });
  }
});
