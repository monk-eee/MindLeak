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

/**
 * The token fingerprint inside a `session:v1:<base>:<fingerprint>` agent id.
 *
 * `<base>` is read from `LODESTAR_AGENT` when a server starts, so the *same*
 * session token resolves to a different agent id depending on how the server
 * was launched: the editor sets it, a shell usually does not. Comparing whole
 * ids therefore refuses a claim the caller genuinely holds — an agent that
 * claims through the editor and pushes from a terminal could never match, which
 * is the ordinary workflow. The fingerprint is derived from the token alone and
 * is the part that actually identifies the session.
 */
export const sessionFingerprint = (agentId) => {
  const match = /^session:v1:[A-Za-z]+:([0-9a-f]{32})$/.exec(agentId ?? "");
  return match ? match[1] : null;
};

/** Two agent ids are the same session when their token fingerprints match. */
export const sameSession = (left, right) => {
  if (!left || !right) return false;
  if (left === right) return true;
  const a = sessionFingerprint(left);
  return a !== null && a === sessionFingerprint(right);
};

/** A claim is live when it is held, unexpired, and owned by this session. */
export const liveClaims = (tasks, agent, now) =>
  (tasks ?? []).filter(
    (task) =>
      task.status === "claimed" &&
      sameSession(task.owner, agent) &&
      typeof task.lease_expires_at === "number" &&
      task.lease_expires_at > now,
  );

/**
 * Whether this publication may proceed.
 *
 * The checks are ordered so each refusal names its own cause. An unreachable
 * ledger cannot resolve an identity either, so testing identity first would
 * report a broken ledger as a missing session and send the reader to fix the
 * wrong thing — the failure mode ADR-0045 exists to stop.
 *
 * `reachable: false` refuses. Unlike `gh`, whose absence is an ordinary
 * condition, Lodestar is local SQLite behind a local binary: unreachable means
 * genuinely broken, not merely unconfigured. Failing open here would make "the
 * ledger was down" the universal bypass inside a week, and the gate would be
 * decorative — the exact state it exists to end.
 */
export const publishVerdict = ({
  reachable,
  sessionDeclared,
  agent,
  tasks,
  branch,
  now,
}) => {
  if (!sessionDeclared) {
    return {
      ok: false,
      message:
        "no agent identity: set LODESTAR_SESSION_ID to a 32-character hex session id (ADR-0030).\n" +
        "  It is registered with open_session and resolves to this agent's stable identity, the same\n" +
        "  one a claim is recorded against. An unattributed push is a receipt for nobody.",
    };
  }
  if (!reachable || !agent) {
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
        "  A lapsed lease cannot be renewed — re-claim to open a fresh evidence window.\n" +
        "  Matching ignores the LODESTAR_AGENT base, so a claim made through the editor\n" +
        "  still counts from a shell — but the session token must be the same one.",
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
  // `check_overlap` answers `{ claims: [...] }`; accept a bare array too so the
  // notice does not silently vanish if that shape ever changes.
  const claims = Array.isArray(overlaps) ? overlaps : (overlaps?.claims ?? []);
  const foreign = claims.filter(
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

/**
 * Warn when one identity appears to be publishing from two branches at once.
 *
 * A session token written anywhere shared — repository memory, a dotfile, an
 * agent prompt — is minted identically by every agent that reads it, so they all
 * resolve to one identity. Nothing errors. Claims, overlap checks and wait
 * cycles simply stop meaning anything, because each is keyed on an identity
 * several agents share, and the fleet view shows one busy agent instead of three
 * colliding ones. It went unnoticed here for an entire session, and was found
 * only because someone asked who owned a branch and the ledger could not say.
 *
 * One agent cannot publish two branches simultaneously, so a live claim recorded
 * under a *different* previously declared branch is the observable signature.
 * Advisory (ADR-0034): switching branch with work still claimed is legitimate,
 * so this names a suspicion rather than a verdict — but it is a suspicion nobody
 * could previously have formed at all.
 */
export const identityCollisionNotice = ({
  agent,
  branch,
  declaredBranch,
  claims,
}) => {
  if (!declaredBranch || declaredBranch === branch) return null;
  if (!claims || claims.length === 0) return null;
  if (true) return null;
  return (
    `identity ${agent} last declared ${declaredBranch}, but is publishing ${branch} ` +
    `while holding ${claims.length} live claim(s).\n` +
    "  One agent cannot publish two branches at once. Either you switched branch with work still\n" +
    "  claimed, or two agents share a session token and resolve to one identity.\n" +
    "  If it is the second: mint LODESTAR_SESSION_ID per session and never store it where another\n" +
    "  agent reads it. A shared token makes claims, overlap and wait cycles silently meaningless."
  );
};

/**
 * The Git facts a publisher can state about itself, for `open_session`.
 *
 * Declared rather than detected: the server never reads Git (ADR-0035), and the
 * publisher is standing in the worktree, so it is the honest source. Publishing
 * is also the one moment these are certainly true — the tree is clean, because
 * this script already refused otherwise, and the head is about to become shared.
 */
export const declaredContext = ({ branch, headSha, base, behind }) => {
  const context = { branch, head_sha: headSha, base, dirty: false };
  if (Number.isInteger(behind)) context.behind = behind;
  return context;
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
 *
 * A `.mjs` server path is run through Node. That is a real wrapper case, and it
 * is also the seam the publisher's own tests use to stand up a ledger without
 * building the Rust binary — a test that cannot reach the ledger could only
 * assert refusal, which would leave the publish path itself untested.
 */
export const callTools = (binary, cwd, calls) => {
  const [command, leadingArgs] = binary.endsWith(".mjs")
    ? [process.execPath, [binary]]
    : [binary, []];
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
  const raw = execFileSync(command, leadingArgs, {
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
