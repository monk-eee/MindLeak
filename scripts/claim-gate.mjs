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
import { existsSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

/**
 * Two agent ids are the same session when they are the same string.
 *
 * This used to compare only the trailing token fingerprint, because the id was
 * `session:v1:<base>:<fingerprint>` and `<base>` came from `LODESTAR_AGENT` read
 * when a *server* started — so one session token resolved to two ids depending
 * on whether the editor or a shell hosted it, and whole-id matching refused a
 * claim the caller genuinely held.
 *
 * ADR-0054 removed the label from the id and migrated every stored identity, so
 * the id is the fingerprint and equality is the whole comparison. Keeping the
 * looser match would be a shim for a bug that no longer exists, and a loose
 * identity comparison is not something to leave lying around: it is the one
 * check standing between an agent and someone else's claim.
 */
export const sameSession = (left, right) => Boolean(left) && left === right;

/** The token fingerprint of an agent id, whichever id shape it is written in. */
const fingerprintOf = (id) => /([0-9a-f]{32})$/.exec(id ?? "")?.[1] ?? null;

/**
 * A claim held by *this* session under a different id shape.
 *
 * Not a fallback, and deliberately not fed into `liveClaims`: this never lets a
 * publication proceed. It exists so the refusal can name its real cause.
 *
 * ADR-0054 collapsed `session:v1:<label>:<fingerprint>` to
 * `session:v1:<fingerprint>` and migrated every stored owner. A server binary
 * built before that still resolves the labelled form, so it asks the ledger
 * about an identity the ledger no longer holds. Every guard downstream is then
 * correct and useless: the claim gate says "no live claim", `claim_task` returns
 * `won: false` on a task this session already owns, and the overlap notice
 * blames a peer for the caller's own work. Three different lies, one stale
 * binary — and nothing in any of those messages points at it.
 *
 * Matching on the fingerprint identifies that case precisely, because the
 * fingerprint is derived from the session token and is the one part both id
 * shapes share.
 */
export const claimsUnderAnotherIdShape = (tasks, agent, now) => {
  const mine = fingerprintOf(agent);
  if (!mine) return [];
  return (tasks ?? []).filter(
    (task) =>
      task.status === "claimed" &&
      !sameSession(task.owner, agent) &&
      fingerprintOf(task.owner) === mine &&
      typeof task.lease_expires_at === "number" &&
      task.lease_expires_at > now,
  );
};

/**
 * The advice to print when this session holds no claim.
 *
 * Says "update your MCP binary" only when the ledger actually shows this
 * session's fingerprint under a different id shape. Guessing at a stale binary
 * every time a claim is missing would teach readers to ignore the line, which
 * is how a diagnostic becomes noise.
 */
export const missingClaimAdvice = (tasks, agent, now) => {
  const shifted = claimsUnderAnotherIdShape(tasks, agent, now);
  if (shifted.length === 0) {
    return "claim a task before publishing.";
  }
  return (
    `this session resolves as ${agent}, but the ledger holds ` +
    `${shifted.length} live claim(s) for the same session token under ` +
    `${shifted[0].owner}. Same session, older id shape: your MCP binary ` +
    "predates ADR-0054, which collapsed the label out of the agent id. " +
    "Rebuild and reinstall the MCP binaries; re-claiming will not help, and " +
    "the claim you already hold is intact."
  );
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
        "  " +
        missingClaimAdvice(tasks, agent, now),
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
 *
 * Ownership is decided by comparing the claim's owner to this session, not only
 * by whether the task appears in a list the caller derived. Observed: an agent's
 * own claim was reported as "another agent has a live claim" because identity
 * resolution had drifted, so the derived list came back empty while the claim
 * itself was plainly its own. A warning that names the wrong party is worse than
 * none — it sends someone to ask a question of themselves, and it trains readers
 * to discount the next one.
 */
export const overlapNotice = (overlaps, ownTaskIds = [], agent = "") => {
  // `check_overlap` answers `{ claims: [...] }`; accept a bare array too so the
  // notice does not silently vanish if that shape ever changes.
  const claims = Array.isArray(overlaps) ? overlaps : (overlaps?.claims ?? []);
  const foreign = claims.filter(
    (overlap) =>
      !ownTaskIds.includes(overlap.task_id) &&
      !sameSession(overlap.owner, agent),
  );
  if (foreign.length === 0) return null;
  const lines = foreign.map((overlap) => {
    const matched = [
      ...(overlap.matching_paths ?? []),
      ...(overlap.matching_symbols ?? []),
    ];
    return `    ${overlap.owner} holds ${overlap.task_id} over ${matched.join(", ")}`;
  });
  // Only claim it is someone else when this session is actually known. With no
  // identity in hand the honest statement is that a claim exists and whose it is
  // cannot be determined here.
  const headline = agent
    ? "another agent has a live claim over paths this branch touches:"
    : "a live claim covers paths this branch touches, and this session has no identity to compare it against:";
  return (
    headline +
    "\n" +
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

const serverBinaries = {
  lodestar: { override: "LODESTAR_MCP_BIN", name: "lodestar-mcp" },
  mindleak: { override: "MINDLEAK_MCP_BIN", name: "mindleak-mcp" },
};

/**
 * Locate a plane's server binary, or `null` when none is built.
 *
 * Both planes resolve the same way deliberately: a caller that can reach the
 * ledger but not the graph would record intent and drop the evidence for it,
 * which is the exact asymmetry that leaves published work uncertifiable.
 */
export const resolveServer = (repoRoot, plane = "lodestar") => {
  const { override, name } = serverBinaries[plane];
  const binaryName = process.platform === "win32" ? `${name}.exe` : name;
  if (process.env[override]) {
    return existsSync(process.env[override]) ? process.env[override] : null;
  }
  for (const profile of ["release", "debug"]) {
    const candidate = join(repoRoot, "target", profile, binaryName);
    if (existsSync(candidate)) return candidate;
  }
  return null;
};

/**
 * Warn when a locally built server predates the source it was built from.
 *
 * A running server is a *build*, not the code in front of you, and the gap
 * between them is invisible: the tool answers, the answer is wrong in exactly
 * the way the old code was wrong, and the obvious next move is to doubt the fix
 * rather than the binary. It cost three separate diagnoses in one day — a
 * conformance verdict read as a product bug, a knowledge record that stayed
 * silent after the fix that should have made it speak, and ADR-0054's whole
 * different-id-shape incident.
 *
 * Only local builds are judged. A binary supplied through the override may be a
 * released artifact that was never built from this tree, and warning that a
 * shipped release is "older than crates/" would be noise on every run — a
 * warning that is always on is a warning nobody reads.
 *
 * Pure so the comparison can be tested without a filesystem: the decision is
 * two numbers and a path, and that is exactly what is easy to get wrong.
 */
export const staleServerNotice = ({
  binary,
  repoRoot,
  binaryMtime,
  sourceMtime,
}) => {
  if (!binary || !binary.startsWith(join(repoRoot, "target"))) return null;
  if (!(sourceMtime > binaryMtime)) return null;
  const behind = Math.round((sourceMtime - binaryMtime) / 1000);
  return (
    `${binary} is ${behind}s older than the source it was built from; it is answering with the previous build.\n` +
    "  Rebuild before trusting what it says:  cargo build -p lodestar-mcp -p mindleak-mcp"
  );
};

/** Newest mtime among the Rust sources a server binary is built from. */
export const newestSourceMtime = (repoRoot) => {
  let newest = 0;
  const walk = (dir) => {
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const full = join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.name.endsWith(".rs")) {
        const { mtimeMs } = statSync(full);
        if (mtimeMs > newest) newest = mtimeMs;
      }
    }
  };
  walk(join(repoRoot, "crates"));
  return newest;
};

/** The notice for a resolved binary, or `null` when it is current or external. */
export const staleServerWarning = (binary, repoRoot) => {
  if (!binary || !binary.startsWith(join(repoRoot, "target"))) return null;
  let binaryMtime;
  try {
    binaryMtime = statSync(binary).mtimeMs;
  } catch {
    return null;
  }
  return staleServerNotice({
    binary,
    repoRoot,
    binaryMtime,
    sourceMtime: newestSourceMtime(repoRoot),
  });
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
