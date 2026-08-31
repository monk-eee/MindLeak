import assert from "node:assert/strict";
import { test } from "node:test";

import {
  branchCommittedPaths,
  runDirectCalls,
  withBranchCommittedPaths,
} from "./mcp-direct.mjs";

const claimCall = (extra = {}) => ({
  name: "task_claim",
  arguments: { task_id: "task:abc", step: "claim", ...extra },
});

test("withBranchCommittedPaths declares what the branch already carries on a claim", () => {
  const seen = {};
  const [call] = withBranchCommittedPaths([claimCall()], {
    repoRoot: "/repo",
    computePaths: (base, options) => {
      seen.base = base;
      seen.repoRoot = options.repoRoot;
      return ["src/a.rs", "docs/b.md"];
    },
  });
  assert.deepEqual(call.arguments.branch_committed_paths, [
    "src/a.rs",
    "docs/b.md",
  ]);
  assert.equal(seen.base, "origin/main", "falls back to origin/main");
  assert.equal(seen.repoRoot, "/repo");
});

test("withBranchCommittedPaths measures from the base the batch itself declared", () => {
  let seenBase = null;
  withBranchCommittedPaths(
    [
      { name: "open_session", arguments: { base: "origin/release-2" } },
      claimCall(),
    ],
    {
      computePaths: (base) => {
        seenBase = base;
        return [];
      },
    },
  );
  assert.equal(seenBase, "origin/release-2");
});

/// Regression: only `step: "claim"` may be enriched.
///
/// THE BUG THIS PREVENTS. `task_claim` is four operations behind one name, and
/// ADR-0147 defines `branch_committed_paths` for exactly one of them. Enriching
/// a `renew` -- which fires repeatedly through a long task -- would attach a
/// growing branch diff to a call that has no use for it, and would run a `git
/// diff` on every heartbeat.
test("withBranchCommittedPaths leaves renew, release and recover untouched", () => {
  for (const step of ["renew", "release", "recover"]) {
    const calls = [
      { name: "task_claim", arguments: { task_id: "task:abc", step } },
    ];
    const result = withBranchCommittedPaths(calls, {
      computePaths: () => ["should-not-be-used"],
    });
    assert.deepEqual(result, calls, `${step} must not be enriched`);
  }
});

/// Regression: the caller's own declaration always wins, including `[]`.
///
/// THE BUG THIS PREVENTS. ADR-0147 decision 1 makes this a caller declaration,
/// not a fact computed on its behalf. An explicit empty array is a real answer
/// ("this branch carries nothing inherited"), so treating it as "unset" and
/// overwriting it would silently replace what the caller asserted with what
/// this script guessed -- the exact substitution the ADR rejects.
test("withBranchCommittedPaths never overrides a caller-supplied declaration", () => {
  const supplied = withBranchCommittedPaths(
    [claimCall({ branch_committed_paths: ["only/mine.rs"] })],
    { computePaths: () => ["computed.rs"] },
  );
  assert.deepEqual(supplied[0].arguments.branch_committed_paths, [
    "only/mine.rs",
  ]);

  const explicitlyEmpty = withBranchCommittedPaths(
    [claimCall({ branch_committed_paths: [] })],
    {
      computePaths: () => ["computed.rs"],
    },
  );
  assert.deepEqual(explicitlyEmpty[0].arguments.branch_committed_paths, []);
});

/// Regression: an unknown answer degrades to silence, never to `[]`.
///
/// THE BUG THIS PREVENTS. If git is unavailable or the base is unknown, the
/// honest report is "no declaration". Substituting an empty array would assert
/// that the branch carries no inherited work -- a confident wrong answer, and
/// worse than saying nothing, because ADR-0147 decision 5 keeps the whole
/// mechanism advisory precisely so an absent input changes nothing.
test("withBranchCommittedPaths forwards the call unchanged when the diff fails", () => {
  const calls = [claimCall()];
  const result = withBranchCommittedPaths(calls, {
    computePaths: () => null,
  });
  assert.deepEqual(result, calls);
  assert.equal("branch_committed_paths" in result[0].arguments, false);
});

test("branchCommittedPaths returns null rather than an empty list when git fails", () => {
  const paths = branchCommittedPaths("origin/main", {
    repoRoot: "/repo",
    runGit: () => {
      throw new Error("git: command not found");
    },
  });
  assert.equal(paths, null);
});

test("branchCommittedPaths splits the diff and drops blank lines", () => {
  const seen = {};
  const paths = branchCommittedPaths("origin/main", {
    repoRoot: "/repo",
    runGit: (args, cwd) => {
      seen.args = args;
      seen.cwd = cwd;
      return "src/a.rs\r\ndocs/b.md\n\n";
    },
  });
  assert.deepEqual(paths, ["src/a.rs", "docs/b.md"]);
  assert.deepEqual(seen.args, ["diff", "--name-only", "origin/main...HEAD"]);
  assert.equal(seen.cwd, "/repo");
});

test("runDirectCalls resolves the plane's binary once and forwards every call in one batch", () => {
  const seen = {};
  const result = runDirectCalls(
    "lodestar",
    [{ name: "open_session", arguments: { a: 1 } }],
    {
      repoRoot: "/repo",
      resolveServerFn: (root, plane) => {
        seen.root = root;
        seen.plane = plane;
        return "/repo/target/release/lodestar-mcp.exe";
      },
      callToolsFn: (binary, cwd, calls) => {
        seen.binary = binary;
        seen.cwd = cwd;
        seen.calls = calls;
        return [{ ok: true }];
      },
    },
  );
  assert.equal(seen.root, "/repo");
  assert.equal(seen.plane, "lodestar");
  assert.equal(seen.binary, "/repo/target/release/lodestar-mcp.exe");
  assert.deepEqual(seen.calls, [{ name: "open_session", arguments: { a: 1 } }]);
  assert.deepEqual(result, [{ ok: true }]);
});

test("runDirectCalls refuses clearly when no binary is found for the plane", () => {
  assert.throws(
    () =>
      runDirectCalls("mindleak", [], {
        repoRoot: "/repo",
        resolveServerFn: () => null,
      }),
    /no mindleak binary found under \/repo\/target/,
  );
});

test("runDirectCalls defaults repoRoot to the current working directory", () => {
  let seenRoot = null;
  assert.throws(() =>
    runDirectCalls("mindleak", [], {
      resolveServerFn: (root) => {
        seenRoot = root;
        return null;
      },
    }),
  );
  assert.equal(seenRoot, process.cwd());
});

test("runDirectCalls enriches a lodestar claim batch before forwarding it", () => {
  let forwarded = null;
  runDirectCalls("lodestar", [claimCall()], {
    repoRoot: "/repo",
    resolveServerFn: () => "/repo/target/release/lodestar-mcp.exe",
    callToolsFn: (_binary, _cwd, calls) => {
      forwarded = calls;
      return [];
    },
    enrichFn: (calls) =>
      calls.map((call) => ({
        ...call,
        arguments: { ...call.arguments, branch_committed_paths: ["a.rs"] },
      })),
  });
  assert.deepEqual(forwarded[0].arguments.branch_committed_paths, ["a.rs"]);
});

/// The Memory Plane has no claims, so it must never be handed a claim-shaped
/// enrichment -- and must never pay for a `git diff` it has no use for.
test("runDirectCalls does not enrich a mindleak batch", () => {
  let enrichCalled = false;
  let forwarded = null;
  const calls = [claimCall()];
  runDirectCalls("mindleak", calls, {
    repoRoot: "/repo",
    resolveServerFn: () => "/repo/target/release/mindleak-mcp.exe",
    callToolsFn: (_binary, _cwd, forwardedCalls) => {
      forwarded = forwardedCalls;
      return [];
    },
    enrichFn: () => {
      enrichCalled = true;
      return [];
    },
  });
  assert.equal(enrichCalled, false);
  assert.deepEqual(forwarded, calls);
});
