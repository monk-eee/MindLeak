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
//   failure path here exits 0 and says nothing.
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
  })();

  // A hook that hangs is a hook that gets uninstalled. Give up quietly.
  await Promise.race([done, new Promise((r) => setTimeout(r, 5000))]);
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
