// Claim gate (ADR-0048). Publication requires a live claim.
//
// Lodestar had one real arbiter (`claim_task`) and zero automatic integration
// points: nothing in the hooks, the scripts, or CI ever consulted it. That does
// not make it bypassed, it makes it optional by construction — and one night of
// nine merged pull requests produced 61 abandoned tasks, two claim owners across
// twenty-three agent identities, and no receipts at all.
//
// The gate is at publication rather than commit on purpose. A commit is a draft:
// cheap, frequent, exploratory. Gate those and people invent tasks to get past
// the check, which fills the board with plans written after the work — a lying
// ledger, which is worse than an empty one because it reads as governed. A push
// is the moment work becomes visible to the rest of the fleet, one branch is an
// honest unit of work, and it is exactly where the coordination failures happen.

import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";

/** A claim is live when it is held, unexpired, and owned by this agent. */
export const liveClaims = (tasks, agent, now) =>
  (tasks ?? []).filter(
    (task) =>
      task.status === "claimed" &&
      task.owner === agent &&
      typeof task.lease_expires_at === "number" &&
      task.lease_expires_at > now,
  );

/**
 * Whether this publication may proceed.
 *
 * `reachable: false` refuses. Unlike `gh`, whose absence is an ordinary
 * condition, Lodestar is local SQLite behind a local binary: unreachable means
 * genuinely broken, not merely unconfigured. Failing open here would make "the
 * ledger was down" the universal bypass inside a week, and the gate would be
 * decorative — the exact state it exists to end.
 */
export const publishVerdict = ({ reachable, agent, tasks, branch, now }) => {
  if (!agent) {
    return {
      ok: false,
      message:
        "no agent identity: set LODESTAR_AGENT so the claim can be attributed (ADR-0030).\n" +
        "  Publication is recorded against an agent; an unattributed push is a receipt for nobody.",
    };
  }
  if (!reachable) {
    return {
      ok: false,
      message:
        "the Lodestar ledger is unreachable, so this publication cannot be attributed to a claim.\n" +
        "  Build it:  cargo build --release\n" +
        "  Or point at an existing server with LODESTAR_MCP_BIN.\n" +
        "  This refuses rather than waves through: an unreachable ledger must not become the way past the gate.",
    };
  }
  const held = liveClaims(tasks, agent, now);
  if (held.length === 0) {
    return {
      ok: false,
      message:
        `no live Lodestar claim for ${agent}; publishing ${branch} would leave no record of what this work was for.\n` +
        "  Claim existing work:  claim_task(task_id)\n" +
        "  Or declare it first:  create_task(goal_id, title, acceptance) then claim_task\n" +
        "  A lapsed lease cannot be renewed — re-claim to open a fresh evidence window.",
    };
  }
  return { ok: true, claims: held };
};

/**
 * Advisory overlap notice, or `null`.
 *
 * Reported, never enforced. Two branches legitimately touch one file, and a
 * gate that refused would be wrong far more often than right; the value is that
 * a human sees the collision at the one moment it is still cheap — before the
 * work is published and two agents have built the same thing twice.
 */
export const overlapNotice = (overlaps, ownTaskIds = []) => {
  const foreign = (overlaps ?? []).filter(
    (overlap) => !ownTaskIds.includes(overlap.task_id),
  );
  if (foreign.length === 0) return null;
  const lines = foreign.map((overlap) => {
    const matched = [
      ...(overlap.matching_paths ?? []),
      ...(overlap.matching_symbols ?? []),
    ];
    return `    ${overlap.owner} holds ${overlap.task_id} over ${matched.join(", ")}`;
  });
  return (
    "another agent has a live claim over paths this branch touches:\n" +
    lines.join("\n") +
    "\n  Publishing anyway is fine; building the same thing twice is not. Ask them (ask_question) before you both land."
  );
};

const binaryName =
  process.platform === "win32" ? "lodestar-mcp.exe" : "lodestar-mcp";

/** Locate a Lodestar server binary, or `null` when none is built. */
export const resolveServer = (repoRoot) => {
  if (process.env.LODESTAR_MCP_BIN) {
    return existsSync(process.env.LODESTAR_MCP_BIN)
      ? process.env.LODESTAR_MCP_BIN
      : null;
  }
  for (const profile of ["release", "debug"]) {
    const candidate = join(repoRoot, "target", profile, binaryName);
    if (existsSync(candidate)) return candidate;
  }
  return null;
};

/**
 * Drive one batch of tool calls over the server's newline-delimited JSON-RPC
 * stdio (not Content-Length framed), returning the parsed results by id.
 */
export const callTools = (binary, cwd, calls) => {
  const requests = [
    {
      jsonrpc: "2.0",
      id: 0,
      method: "initialize",
      params: {
        protocolVersion: "2024-11-05",
        capabilities: {},
        clientInfo: { name: "canonical-push", version: "1" },
      },
    },
    ...calls.map((call, index) => ({
      jsonrpc: "2.0",
      id: index + 1,
      method: "tools/call",
      params: { name: call.name, arguments: call.arguments ?? {} },
    })),
  ];
  const raw = execFileSync(binary, [], {
    cwd,
    encoding: "utf8",
    input: requests.map((request) => JSON.stringify(request)).join("\n") + "\n",
    stdio: ["pipe", "pipe", "pipe"],
    timeout: 30_000,
  });
  const byId = new Map();
  for (const line of raw.split(/\r?\n/)) {
    if (!line.trim()) continue;
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      continue;
    }
    if (message.id === undefined || !message.result) continue;
    const text = message.result?.content?.[0]?.text;
    if (text === undefined) continue;
    try {
      byId.set(message.id, JSON.parse(text));
    } catch {
      byId.set(message.id, text);
    }
  }
  return calls.map((_, index) => byId.get(index + 1));
};
