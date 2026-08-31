// Drives Lodestar/MindLeak tool calls directly against the built release
// binaries over the same newline-delimited JSON-RPC stdio
// scripts/canonical-push.mjs already speaks, bypassing a persistent editor
// MCP connection entirely.
//
// Exists because that persistent connection can break for one client session
// and not recover on its own, even across a window reload -- see
// gaps.d/mcp-server-processes-accumulate-per-editor-window.md -- while the
// underlying binaries and their SQLite-backed state are otherwise completely
// unaffected: canonical-push.mjs already drives a fresh instance of them over
// stdio on every publish, regardless of whether any editor's own MCP
// connection is healthy. This is that same mechanism made runnable on its
// own, so a session that has lost its editor connection is not also
// forced to skip claiming, checking overlap, or recording evidence.
//
// It is also the caller ADR-0147 decision 6 names for `branch_committed_paths`:
// a claim taken here declares what the branch already carries, so Lodestar can
// report the clauses governing that inherited work separately from the task's
// own scope. Without a caller supplying it, that parameter is unreachable.
//
// A batch of calls MUST be one invocation: each server process is stateless
// across invocations (session state lives only in that one process's
// memory), so `open_session` and everything that depends on it have to run
// in the same call to see it.
//
// Usage:
//   node scripts/mcp-direct.mjs <lodestar|mindleak> <calls.json>
// where calls.json is `[{"name": "...", "arguments": {...}}, ...]`, the same
// shape scripts/claim-gate.mjs's callTools already takes.

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

import { callTools, resolveServer } from "./claim-gate.mjs";

/** The base a claim's branch diff is measured from when the batch declares one. */
const DEFAULT_BASE = "origin/main";

/**
 * The paths already committed on this branch since `base`, as ADR-0147
 * decision 1 defines them -- the same `git diff --name-only <base>...HEAD`
 * canonical-push.mjs computes at publish time, run at claim time instead.
 *
 * Returns null, never `[]`, when the answer is unknown: git missing, an
 * unknown base, or a detached/odd checkout. ADR-0147 decision 4 says an
 * absent declaration degrades to exactly today's behaviour, and an empty
 * array is not absence -- it is a positive claim that the branch carries
 * nothing, which is precisely the false reassurance the ADR warns against.
 */
export function branchCommittedPaths(base, { repoRoot, runGit } = {}) {
  try {
    const output = runGit(["diff", "--name-only", `${base}...HEAD`], repoRoot);
    return output.split(/\r?\n/).filter(Boolean);
  } catch {
    return null;
  }
}

function defaultRunGit(args, repoRoot) {
  return execFileSync("git", args, { cwd: repoRoot, encoding: "utf8" });
}

/** The base this batch itself declared, so the diff matches the session's own claim. */
function declaredBase(calls) {
  const session = calls.find((call) => call?.name === "open_session");
  const base = session?.arguments?.base;
  return typeof base === "string" && base.trim() ? base.trim() : DEFAULT_BASE;
}

/**
 * Attach `branch_committed_paths` to a `task_claim` that is taking a claim
 * (ADR-0147 decision 6): this is the documented direct-drive caller, and
 * without a caller supplying it the parameter slices 1-2 added is unreachable.
 *
 * Deliberately narrow. Only `step: "claim"` is enriched, because that is the
 * only step the argument is defined for. A caller that already supplied the
 * key keeps its own value untouched -- including an explicit `[]` -- since
 * decision 1 makes this the caller's declaration, not something computed on
 * its behalf and overridden.
 */
export function withBranchCommittedPaths(
  calls,
  {
    repoRoot,
    runGit = defaultRunGit,
    computePaths = branchCommittedPaths,
  } = {},
) {
  const needsPaths = calls.some(
    (call) =>
      call?.name === "task_claim" &&
      call?.arguments?.step === "claim" &&
      !("branch_committed_paths" in call.arguments),
  );
  if (!needsPaths) {
    return calls;
  }
  const paths = computePaths(declaredBase(calls), { repoRoot, runGit });
  if (paths === null) {
    return calls;
  }
  return calls.map((call) => {
    if (
      call?.name !== "task_claim" ||
      call?.arguments?.step !== "claim" ||
      "branch_committed_paths" in call.arguments
    ) {
      return call;
    }
    return {
      ...call,
      arguments: { ...call.arguments, branch_committed_paths: paths },
    };
  });
}

/** Resolve the plane's binary once, then forward every call to it as one batch. */
export function runDirectCalls(
  plane,
  calls,
  {
    repoRoot = process.cwd(),
    resolveServerFn = resolveServer,
    callToolsFn = callTools,
    enrichFn = withBranchCommittedPaths,
  } = {},
) {
  const binary = resolveServerFn(repoRoot, plane);
  if (!binary) {
    throw new Error(`no ${plane} binary found under ${repoRoot}/target`);
  }
  const enriched = plane === "lodestar" ? enrichFn(calls, { repoRoot }) : calls;
  return callToolsFn(binary, repoRoot, enriched, 16 * 1024 * 1024);
}

if (import.meta.filename === process.argv[1]) {
  const [, , plane, callsFile] = process.argv;
  if (!plane || !callsFile) {
    console.error(
      "usage: node scripts/mcp-direct.mjs <lodestar|mindleak> <calls.json>",
    );
    process.exit(1);
  }
  const calls = JSON.parse(readFileSync(callsFile, "utf8"));
  const results = runDirectCalls(plane, calls);
  console.log(JSON.stringify(results, null, 2));
}
