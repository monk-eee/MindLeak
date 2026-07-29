#!/usr/bin/env node
// Record the commit as evidence at the moment it is made.
//
// Measured on this repository: forty conformance audits reported "evidence
// contains no provenance-bearing mutation", sixteen of them after the argument
// guard that was meant to have fixed that. Every bundle was empty. The cause is
// not carelessness -- it is that `ingest_commit` has to be *remembered*, at a
// moment when the work feels finished and attention has already moved on.
//
// Nothing in this repository that relies on remembering has worked. ADR-0046
// measured zero adoption for a capability that needed a separate call. What has
// worked, without exception, is hanging the obligation off something the agent
// already does: `canonical-push` refuses without a claim and nobody forgets it;
// arming a pull request *is* joining the delivery queue (ADR-0045/0062).
//
// Committing is the thing every agent already does. So evidence exists because
// you committed, not because you remembered -- and the empty bundle stops being
// possible rather than being caught later.
//
// TWO RULES, BOTH LOAD-BEARING:
//
//   It never fails a commit. Evidence capture that can block work would be
//   disabled within a day, and then the graph is worse off than before. Every
//   failure path here exits 0 -- but it says so on the way out. Silence was the
//   original design and it cost a real investigation: a commit landed with no
//   provenance, the bundle came back empty, and nothing connected that to a hook
//   that had timed out minutes earlier. Never blocking and never reporting are
//   different promises; only the first one is load-bearing.
//
//   It records only what git already knows -- sha, subject, changed paths, and
//   the commit's own timestamp. No interpretation, no model, no tokens. That
//   keeps it on the deterministic ingest path (invariant 1).

import { execFileSync, spawn } from "node:child_process";
import { createInterface } from "node:readline";

/** Facts about the commit just made, straight from git. */
export function readCommit(run) {
  const [sha, at, subject] = run([
    "log",
    "-1",
    "--format=%H%x00%ct%x00%s",
  ]).split("\0");
  const changed = run(["show", "--name-only", "--format=", sha])
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
  return { sha, timestamp: Number(at), message: subject, changed };
}

/**
 * Whether this commit is worth ingesting at all.
 *
 * A merge commit is not new work -- its content already arrived on the branches
 * it joins, and ingesting it would attribute every file in the merge to
 * whoever happened to run it. An empty commit has nothing to attribute.
 */
export function worthIngesting(commit, parentCount) {
  if (parentCount > 1) return false;
  return commit.changed.length > 0;
}

/**
 * How long to wait before giving up. A hook that hangs is a hook that gets
 * uninstalled, so the budget stays small; it is configurable because a loaded
 * machine can spend most of it just starting the server binary.
 */
export const timeoutMs = (env = process.env) => {
  const raw = Number(env.MINDLEAK_INGEST_TIMEOUT_MS);
  return Number.isFinite(raw) && raw > 0 ? raw : 5000;
};

/**
 * What to say when provenance was not recorded.
 *
 * Giving up quietly was the original design, and it was wrong: a commit landed
 * with no provenance, the evidence bundle came back empty, and the task was
 * uncertifiable -- with nothing anywhere connecting that outcome back to a hook
 * that had silently timed out minutes earlier. Diagnosing it cost far more than
 * this line ever will. The commit still succeeds; only the silence is fixed.
 */
export function skippedWarning(sha, reason) {
  return (
    `ingest-commit: provenance NOT recorded for ${sha} (${reason}).\n` +
    "  The commit succeeded. Evidence for it will be missing, so a conformance\n" +
    "  check over this window will report an empty bundle.\n" +
    "  Backfill with mindleak `ingest_commit`, passing this commit's OWN timestamp\n" +
    "  (`git log -1 --format=%ct`) -- a node keeps the timestamp it was first given."
  );
}

const client = (bin) => {
  const proc = spawn(bin, [], { stdio: ["pipe", "pipe", "ignore"] });
  const pending = new Map();
  let nextId = 1;
  createInterface({ input: proc.stdout }).on("line", (line) => {
    let m;
    try {
      m = JSON.parse(line);
    } catch {
      return;
    }
    const settle = pending.get(m.id);
    if (!settle) return;
    pending.delete(m.id);
    settle(m.result);
  });
  const send = (method, params) =>
    new Promise((settle) => {
      const id = nextId++;
      pending.set(id, settle);
      proc.stdin.write(
        `${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`,
      );
    });
  return { proc, send };
};

async function main() {
  const bin =
    process.env.MINDLEAK_MCP_BIN ??
    (process.platform === "win32"
      ? "target/release/mindleak-mcp.exe"
      : "target/release/mindleak-mcp");
  const session =
    process.env.LODESTAR_SESSION_ID ?? process.env.MINDLEAK_SESSION_ID;
  // No session means no agent to attribute the work to. A human committing by
  // hand is not a failure and must not be told off for it.
  if (!session) return;

  const run = (args) =>
    execFileSync("git", args, { encoding: "utf8", maxBuffer: 1 << 26 }).trim();
  const commit = readCommit(run);
  const parents =
    run(["rev-list", "--parents", "-n", "1", commit.sha]).split(/\s+/).length -
    1;
  if (!worthIngesting(commit, parents)) return;

  const { proc, send } = client(bin);
  // A missing or unstartable binary must not throw: an unhandled 'error' event
  // on the child would take the hook down with a stack trace the committer has
  // no use for.
  const failed = new Promise((resolve) =>
    proc.on("error", () => resolve("the MindLeak server could not be started")),
  );
  const done = (async () => {
    await send("initialize", {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "post-commit", version: "1" },
    });
    proc.stdin.write(
      `${JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized", params: {} })}\n`,
    );
    await send("tools/call", {
      name: "open_session",
      arguments: { session_id: session },
    });
    await send("tools/call", {
      name: "ingest_commit",
      arguments: {
        sha: commit.sha,
        message: commit.message,
        changed_files: commit.changed,
        timestamp: commit.timestamp,
        session_id: session,
      },
    });
    return null;
  })();

  // Give up rather than hang -- but say so. Losing provenance is cheap to
  // report and expensive to discover later.
  const budget = timeoutMs();
  const reason = await Promise.race([
    done.catch(() => "the MindLeak server did not answer"),
    failed,
    new Promise((r) =>
      setTimeout(() => r(`no response within ${budget}ms`), budget),
    ),
  ]);
  if (reason) console.error(skippedWarning(commit.sha, reason));
  proc.kill();
}

if (
  process.argv[1] &&
  import.meta.url.endsWith(process.argv[1].replace(/\\/g, "/"))
) {
  main()
    .catch(() => {})
    .finally(() => process.exit(0));
}
