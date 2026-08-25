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
 * Commits that landed before any held claim could possibly cover them
 * (gaps.d/commit-then-claim-puts-evidence-before-its-claim.md).
 *
 * `check_conformance` bounds an evidence bundle by `claim_started_at`, so a
 * commit whose own timestamp is earlier than every claim this session holds
 * can never be certified, no matter how the evidence window is drawn
 * afterward — the moment it happened was not authorised by anything. That is
 * a fact about the commit and the earliest claim alone; a later claim cannot
 * retroactively cover it, but an earlier one might, so this only flags a
 * commit that predates ALL held claims, never one merely older than some of
 * them.
 *
 * Deliberately advisory-only and computed here rather than gating on it: the
 * claim gate above already establishes, at ADR-0048's own choosing, that
 * commits stay ungated and only publication requires a claim. Blocking here
 * too would reintroduce exactly the failure the gate's design note warns
 * against — inventing a task after the fact to get past a check.
 */
export const commitsBeforeClaim = (commits, claims) => {
  const starts = (claims ?? [])
    .map((claim) => claim.claim_started_at)
    .filter(Number.isFinite);
  if (starts.length === 0) return [];
  const earliestClaim = Math.min(...starts);
  return (commits ?? []).filter(
    (commit) =>
      typeof commit.timestamp === "number" && commit.timestamp < earliestClaim,
  );
};

/** The advisory printed when `commitsBeforeClaim` finds any. Never blocks. */
export const commitBeforeClaimNotice = (commits) => {
  if (!commits || commits.length === 0) return null;
  const shas = commits.map((commit) => commit.sha.slice(0, 7)).join(", ");
  return (
    `${commits.length} commit(s) on this branch (${shas}) landed before this session's earliest ` +
    "held claim began. check_conformance will report their evidence as outside the claim window no " +
    "matter how the bundle is built afterward " +
    "(gaps.d/commit-then-claim-puts-evidence-before-its-claim.md). This push still succeeds; " +
    "completing the task may need merge_evidence (verifies a commit from git directly, once it has " +
    "reached main) or a human resolve, rather than a hand-built evidence window."
  );
};

/**
 * The finished task this branch belongs to, when the push only reconciles it.
 *
 * Completing a task releases its claim, so a delivered branch could never be
 * brought up to date again: `main` moves, the pull request goes stale, and the
 * delivery queue steps over it forever. Observed on #168, which needed hand
 * reconciliation three times, each one inventing a throwaway task to get past
 * this gate — and inventing a task per republish is exactly how six duplicate
 * tasks reached the board.
 *
 * A branch a task recorded (ADR: a task records the branch it is claimed on) is
 * already attributed. Re-attributing it to a fresh task records a fiction.
 *
 * The narrow part is `newCommits`: every new commit must be a merge. A
 * reconciliation merges the base in and nothing else, so this cannot become
 * "finish a task, then push anything to that branch forever" — the moment real
 * work appears, the exemption stops applying and a claim is required again.
 */
export const reconciliationOf = ({ tasks, branch, newCommits }) => {
  if (!branch || !Array.isArray(newCommits) || newCommits.length === 0) {
    return null;
  }
  if (!newCommits.every((commit) => commit.isMerge)) return null;
  return (
    (tasks ?? []).find(
      (task) =>
        task.branch === branch &&
        (task.status === "done" || task.status === "abandoned"),
    ) ?? null
  );
};

/**
 * Restores what a board fetched with `include_terminal: false` cannot answer
 * on its own: whether THIS branch was already delivered by a terminal task.
 *
 * The general board fetch below stays `include_terminal: false` deliberately
 * (gaps.d/task-query-board-has-no-response-size-bound.md) — pulling in every
 * terminal task the ledger has ever held just to find one row would defeat
 * the whole point of asking narrowly. `board(view="board", branch)` answers
 * the narrow question instead: any status, but only this one branch. Without
 * this merge, `reconciliationOf` can never match anything, because the task
 * it needs to find is a `done`/`abandoned` row that `include_terminal: false`
 * excluded before `reconciliationOf` ever saw it — a bug this function closes,
 * not a variant of the size-bound gap.
 *
 * `primary` wins any id collision (a live claim on this exact branch, for
 * example) so nothing here can make `liveClaims` see a different picture of
 * the same task than the general fetch already gave it.
 */
export const withReconciliationCandidates = (primary, candidates) => {
  const seen = new Set((primary ?? []).map((task) => task.id));
  const extra = (candidates ?? []).filter((task) => !seen.has(task.id));
  return [...(primary ?? []), ...extra];
};

/**
 * Whether this publication may proceed.
 *
 * The checks are ordered so each refusal names its own cause, and the three
 * server-side outcomes are kept distinct because their remedies differ. An
 * unreachable ledger cannot resolve an identity either, so testing identity
 * first would report a broken ledger as a missing session and send the reader
 * to fix the wrong thing — the failure mode ADR-0045 exists to stop.
 *
 *   - `reachable: false` — the binary did not answer. Rebuild it, or point
 *     LODESTAR_MCP_BIN at a running one.
 *   - `agent` empty — it answered but did not identify the session. The ledger
 *     is fine; the session id is the thing to check, not the binary.
 *   - `boardReadable: false` — it answered and identified the session, but the
 *     task board could not be read. The deployed binary is older than the
 *     ledger and cannot parse an event a newer writer recorded; the fix is a
 *     current binary, not a rebuild of a ledger that is not broken. Folding
 *     this into "unreachable" cost a session once: the message offered
 *     `cargo build` for a binary that was present and answering.
 *
 * `reachable: false` refuses. Unlike `gh`, whose absence is an ordinary
 * condition, Lodestar is local SQLite behind a local binary: unreachable means
 * genuinely broken, not merely unconfigured. Failing open here would make "the
 * ledger was down" the universal bypass inside a week, and the gate would be
 * decorative — the exact state it exists to end.
 */
export const unreadableBoardGuidance =
  "rebuilding it will not help: the deployed binary is almost certainly older than the ledger it is\n" +
  "  reading and cannot parse an event a newer writer recorded. Point LODESTAR_MCP_BIN at the shared\n" +
  "  install (~/.mindleak/bin/lodestar-mcp), which tracks the current build.";

export const publishVerdict = ({
  reachable,
  boardReadable,
  sessionDeclared,
  agent,
  tasks,
  branch,
  newCommits,
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
  if (!agent) {
    return {
      ok: false,
      message:
        "the Lodestar ledger answered but did not identify this session, so this publication cannot be\n" +
        "  attributed to a claim. The ledger is reachable, so do not rebuild it: check that\n" +
        "  LODESTAR_SESSION_ID is the same 32-character hex id the claim was made under (ADR-0030), the\n" +
        "  one open_session resolves to a stable agent identity.",
    };
  }
  if (!boardReadable) {
    return {
      ok: false,
      message:
        "the Lodestar ledger answered and identified this session, but its task board could not be read,\n" +
        "  so this push cannot be checked against your claims. This is not an unreachable ledger and\n" +
        `  ${unreadableBoardGuidance}`,
    };
  }
  const held = liveClaims(tasks, agent, now);
  if (held.length === 0) {
    const delivered = reconciliationOf({ tasks, branch, newCommits });
    if (delivered) {
      return {
        ok: true,
        claims: [],
        reconciles: delivered,
        notice:
          `no live claim, but ${branch} was delivered by ${delivered.id} and this push only merges ` +
          "the base in. Publishing as a reconciliation, attributed to that task — a delivered branch\n" +
          "  must stay reconcilable, and minting a task per republish would record a fiction.",
      };
    }
    return {
      ok: false,
      message:
        `no live Lodestar claim for ${agent}; publishing ${branch} would leave no record of what this work was for.\n` +
        '  Claim existing work:  task_claim(task_id, step="claim")\n' +
        "  Or declare it first:  task_create(goal_id, title, acceptance) then task_claim\n" +
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
 * Drive one batch of tool calls over the server's newline-delimited JSON-RPC
 * stdio (not Content-Length framed), returning the parsed results by id.
 *
 * A `.mjs` server path is run through Node. That is a real wrapper case, and it
 * is also the seam the publisher's own tests use to stand up a ledger without
 * building the Rust binary — a test that cannot reach the ledger could only
 * assert refusal, which would leave the publish path itself untested.
 *
 * `maxBuffer` defaults to `execFileSync`'s own 1 MiB default so every existing
 * caller's behaviour is unchanged; a caller reading a view whose payload can
 * grow with the repository's history (`task_query view=board` on a board with
 * hundreds of terminal tasks measured well past 1 MiB) must raise it
 * explicitly rather than this function silently guessing a bigger number for
 * everyone.
 */
export const callTools = (binary, cwd, calls, maxBuffer = 1024 * 1024) => {
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
    maxBuffer,
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
    byId.set(message.id, parseCallResult(message.result));
  }
  return calls.map((_, index) => byId.get(index + 1));
};

/**
 * Parse one `tools/call` result the same way the extension's own
 * `parseToolResult` (editors/vscode/src/util.ts) already does: prefer the
 * machine-readable `structuredContent` a tool renders Markdown-for-chat
 * alongside, falling back to the first text-content block parsed as JSON
 * (or the raw text) for tools that never carry one. Reading only
 * `content[0].text` here silently returned prose instead of data for every
 * tool already migrated to that dual format -- `lodestar_stats` answered
 * with a Markdown table string, not a `{active_goals, ...}` object, and every
 * field read off it was `undefined`.
 */
export function parseCallResult(result) {
  if (
    result?.structuredContent !== undefined &&
    result?.structuredContent !== null
  ) {
    return result.structuredContent;
  }
  const text = result?.content?.[0]?.text;
  if (typeof text !== "string") return undefined;
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}
