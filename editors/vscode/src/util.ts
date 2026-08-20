// Pure, dependency-free helpers (no vscode / fs imports) so they can be unit-tested.
import * as os from "os";
import * as path from "path";

/** Convert a workspace-relative path to a MindLeak artifact node id. */
export function toArtifactId(relPath: string): string {
  return `artifact:${relPath.replace(/\\/g, "/")}`;
}

/**
 * The repository-relative form of a path the editor produced, or `null` when it
 * cannot be placed in this workspace.
 *
 * `vscode.workspace.asRelativePath` returns its input *unchanged* when the file
 * sits outside every workspace folder, and node ids are repo-relative by
 * contract. Agents routinely edit a sibling worktree from a window rooted
 * elsewhere, so that unchanged absolute path used to go on the wire and become a
 * second identity for a file the graph already tracked — one file was measured
 * holding 117 structural edges under its absolute id and 43 under its relative
 * one. The server refuses such a path now; this stops the editor asking.
 *
 * Mirrors the server's rule so the two agree on what "relative" means: a POSIX
 * or UNC root and a Windows drive are absolute, while `./x` and `../x` are
 * relative even though they leave the folder.
 */
export function repoRelativePath(raw: string): string | null {
  const normalized = raw.replace(/\\/g, "/");
  if (normalized === "" || normalized.startsWith("/") || /^[a-zA-Z]:/.test(normalized)) {
    return null;
  }
  return normalized;
}

/** Select the path sent to the server, which owns worktree canonicalisation. */
export function serverFilePath(workspacePath: string, fsPath: string): string | null {
  const relative = repoRelativePath(workspacePath);
  if (relative !== null) {
    return relative;
  }
  const normalized = fsPath.replace(/\\/g, "/");
  return normalized === "" ? null : normalized;
}

/**
 * Parse an MCP tool result. Prefers the machine-readable `structuredContent`
 * (present when a tool renders Markdown for chat but still exposes JSON for
 * programmatic consumers); otherwise parses the first text-content block as JSON,
 * falling back to the raw text (or the whole result) when it is not JSON.
 */
export function parseToolResult(result: unknown): unknown {
  const structured = (result as { structuredContent?: unknown })?.structuredContent;
  if (structured !== undefined && structured !== null) {
    return structured;
  }
  const text = (result as { content?: Array<{ text?: unknown }> })?.content?.[0]?.text;
  if (typeof text !== "string") {
    return result;
  }
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

const SENSITIVE_COMMAND =
  /(?:^|\s)(?:read(?:-host)?|passwd|ssh-add|set\s+\/p|sudo\s+-S|az\s+login|gh\s+auth\s+login|npm\s+login|docker\s+login|git\s+credential)(?:\s|$)|(?:--?(?:password|passphrase|token|api[-_]?key)\b)|(?:password|passphrase|token|api[-_]?key)\s*=|authorization\s*:\s*bearer/i;

/** Whether a shell-integrated command is reliable and safe enough to retain. */
export function shouldCaptureCommand(command: string, confidence: number): boolean {
  return confidence >= 1 && command.trim().length > 0 && !SENSITIVE_COMMAND.test(command);
}

/** Strip terminal controls, redact common secret forms, and cap retained output. */
export function redactTerminalOutput(output: string, maxChars: number): string {
  if (maxChars <= 0) {
    return "";
  }
  const clean = stripTerminalControls(output)
    .replace(/(authorization\s*:\s*bearer\s+)[^\s]+/gi, "$1[REDACTED]")
    .replace(/((?:password|passphrase|token|api[-_]?key)\s*[=:]\s*)[^\s]+/gi, "$1[REDACTED]")
    .replace(/\bAKIA[0-9A-Z]{16}\b/g, "[REDACTED]")
    .replace(/\bgh[pousr]_[A-Za-z0-9]{20,}\b/g, "[REDACTED]");
  return Array.from(clean).slice(0, maxChars).join("");
}

function stripTerminalControls(output: string): string {
  let clean = "";
  for (let index = 0; index < output.length; index += 1) {
    const code = output.charCodeAt(index);
    if (code === 27) {
      const kind = output[index + 1];
      if (kind === "[") {
        index += 2;
        while (index < output.length) {
          const final = output.charCodeAt(index);
          if (final >= 64 && final <= 126) {
            break;
          }
          index += 1;
        }
      } else if (kind === "]") {
        index += 2;
        while (index < output.length) {
          if (output.charCodeAt(index) === 7) {
            break;
          }
          if (output.charCodeAt(index) === 27 && output[index + 1] === "\\") {
            index += 1;
            break;
          }
          index += 1;
        }
      } else {
        index += 1;
      }
      continue;
    }
    if (code < 32 && ![9, 10, 13].includes(code)) {
      continue;
    }
    clean += output[index];
  }
  return clean;
}

/** Normalize, exclude, sort, and cap workspace-relative changed paths. */
export function filterChangedPaths(
  paths: Iterable<string>,
  excludedPrefixes: string[],
  maxFiles = Number.POSITIVE_INFINITY
): string[] {
  const prefixes = excludedPrefixes
    .map((prefix) => prefix.replace(/\\/g, "/").replace(/^\.\//, "").replace(/\/$/, ""))
    .filter(Boolean);
  return [...new Set([...paths].map((file) => file.replace(/\\/g, "/")))]
    .filter((file) => !prefixes.some((prefix) => file === prefix || file.startsWith(`${prefix}/`)))
    .sort()
    .slice(0, Math.max(0, maxFiles));
}

export interface ResolveServerOptions {
  platform?: NodeJS.Platform;
  exists?: (candidate: string) => boolean;
  version?: (candidate: string) => string | undefined;
  extensionPath?: string;
  homeDir?: string;
}

/** Stable, scan-friendly status text for both planes and passive sensors. */
export function healthSummary(
  memory: string,
  intent: string,
  terminal: string,
  git: string
): string {
  return `${memory} · ${intent} · ${terminal} · ${git}`;
}

/** Emit one server environment override only when the user configured a path. */
export function configuredPathEnvironment(
  variable: string,
  configured: string | undefined
): Record<string, string> {
  const value = configured?.trim();
  return value ? { [variable]: value } : {};
}

/**
 * Prefer a workspace-built `mindleak-mcp` binary when the configured path is the
 * bare default. Thin wrapper over {@link resolveBinaryPath}.
 */
export function resolveServerPath(
  configured: string,
  workspace: string,
  opts: ResolveServerOptions = {}
): string {
  return resolveBinaryPath(configured, workspace, "mindleak-mcp", opts);
}

/**
 * Which candidate {@link resolveBinaryPathDetailed} resolved to, in the same
 * priority order it checks them.
 */
export type BinarySource =
  | "explicit-config"
  | "packaged"
  | "shared-install"
  | "workspace-release"
  | "workspace-debug"
  | "fallback";

/** A resolved binary path, and which candidate produced it. */
export interface ResolvedBinary {
  readonly path: string;
  readonly source: BinarySource;
}

/**
 * Prefer the packaged binary, then the shared per-machine install, then a
 * workspace build, when the configured path is the bare default name. Generic
 * over both MCP server binaries; filesystem inputs are injectable so this stays
 * pure and testable.
 *
 * The shared install at `~/.mindleak/bin` outranks a worktree's own
 * `target/release` deliberately. ADR-0073 chose one binary per machine after
 * measuring 56 worktrees holding 184 GB of `target/`, and searching the
 * worktree first would quietly reinstate the per-worktree binary — and its
 * stale-build problem — for every window.
 *
 * Named which candidate won (not only the path) so a stale packaged binary
 * silently outranking a rebuilt one is diagnosable rather than invisible.
 */
export function resolveBinaryPathDetailed(
  configured: string,
  workspace: string,
  binaryName: string,
  opts: ResolveServerOptions = {}
): ResolvedBinary {
  const platform = opts.platform ?? process.platform;
  const exists = opts.exists ?? (() => false);
  if (configured && configured !== binaryName) {
    return { path: configured, source: "explicit-config" };
  }
  const exe = platform === "win32" ? `${binaryName}.exe` : binaryName;
  if (opts.extensionPath) {
    const packaged = path.join(opts.extensionPath, "bin", exe);
    if (exists(packaged)) {
      return { path: packaged, source: "packaged" };
    }
  }
  const home = opts.homeDir ?? os.homedir();
  if (home) {
    const installed = path.join(home, ".mindleak", "bin", exe);
    if (exists(installed)) {
      return { path: installed, source: "shared-install" };
    }
  }
  for (const profile of ["release", "debug"] as const) {
    const candidate = path.join(workspace, "target", profile, exe);
    if (exists(candidate)) {
      return {
        path: candidate,
        source: profile === "release" ? "workspace-release" : "workspace-debug",
      };
    }
  }
  return { path: configured || binaryName, source: "fallback" };
}

/** Thin wrapper over {@link resolveBinaryPathDetailed} for callers that only need the path. */
export function resolveBinaryPath(
  configured: string,
  workspace: string,
  binaryName: string,
  opts: ResolveServerOptions = {}
): string {
  return resolveBinaryPathDetailed(configured, workspace, binaryName, opts).path;
}

/** One MCP server the extension contributes to the editor, resolved and rooted. */
export interface McpServerPlan {
  readonly id: string;
  readonly label: string;
  readonly command: string;
  readonly version?: string;
  readonly cwd: string;
  readonly env: Record<string, string>;
}

/** The server paths and database overrides a window has configured. */
export interface ConfiguredServers {
  readonly memory: string;
  readonly intent: string;
  readonly memoryDatabase?: string;
  readonly intentDatabase?: string;
}

/**
 * Both planes as the editor should launch them for this window.
 *
 * The extension contributes the servers itself rather than a committed
 * `.vscode/mcp.json` naming them, so there is one rule for where a binary lives
 * — {@link resolveBinaryPath}, already covered by tests — instead of a config
 * file carrying a second, untested copy of it that only drifts.
 *
 * Every server is rooted at the workspace folder of the window that provides
 * it, which is what keeps ADR-0073 true: an agent editing a sibling worktree
 * must have its files resolve against *that* worktree, or one file acquires a
 * second identity in the graph.
 *
 * Pure by design: filesystem and platform come in through `opts`, so what the
 * editor would be told is assertable without launching an editor.
 */
export function planMcpServers(
  workspace: string,
  agentId: string,
  configured: ConfiguredServers,
  opts: ResolveServerOptions = {}
): McpServerPlan[] {
  const memoryCommand = resolveBinaryPath(configured.memory, workspace, "mindleak-mcp", opts);
  const intentCommand = resolveBinaryPath(configured.intent, workspace, "lodestar-mcp", opts);
  return [
    {
      id: "mindleak",
      label: "MindLeak memory",
      command: memoryCommand,
      version: opts.version?.(memoryCommand),
      cwd: workspace,
      env: {
        ...configuredPathEnvironment("MINDLEAK_DB", configured.memoryDatabase),
        MINDLEAK_AGENT: agentId,
        MINDLEAK_WORKSPACE: workspace,
      },
    },
    {
      id: "lodestar",
      label: "Lodestar intent",
      command: intentCommand,
      version: opts.version?.(intentCommand),
      cwd: workspace,
      env: {
        ...configuredPathEnvironment("LODESTAR_DB", configured.intentDatabase),
        LODESTAR_AGENT: agentId,
        MINDLEAK_WORKSPACE: workspace,
      },
    },
  ];
}

/** A task from Lodestar `task_query(view=board)` (subset used by the UI). */
export interface LodestarTask {
  id: string;
  goal_id: string;
  title: string;
  acceptance?: string;
  status: string;
  owner?: string | null;
  /**
   * The branch this task's current evidence window is being done on
   * (ADR-0035 d5), pinned at claim time from what the owner declared to
   * `open_session`. `null`/absent when none was declared — surfaced only when
   * present, never guessed.
   */
  branch?: string | null;
  claim_started_at?: number | null;
  lease_expires_at?: number | null;
  blocked_by?: string | null;
  parked_at?: number | null;
  /** Clauses governing this task's scope, when the client has fetched them (ADR-0029). */
  governing?: GoverningClause[];
  scope?: TaskScope;
}

/** One active clause governing a task's scope, from `advise` / `governing_for_task` (ADR-0029). */
export interface GoverningClause {
  node_id: string;
  goal: { id: string; title: string; kind: string };
  mode: string; // "governed" | "forbid_change"
}

/**
 * Render the clauses governing a task as a bounded tooltip section, so a human
 * reading the board sees what governs the work an agent picked up (ADR-0029).
 * Pure and empty-safe: returns "" when nothing governs, so callers can append
 * it unconditionally.
 */
export function formatGoverningClauses(governing: GoverningClause[] | undefined): string {
  if (!governing || governing.length === 0) {
    return "";
  }
  const lines = governing.map(
    (clause) => `\n- ${clause.goal.title} (${clause.goal.kind}, ${clause.mode})`
  );
  return `\n\nGoverned by:${lines.join("")}`;
}

export interface TaskScope {
  paths: string[];
  symbols: string[];
}

export type TaskLeaseState = "claimable" | "live" | "expired" | "parked" | "unavailable";

export interface TaskLeaseRequest {
  task_id: string;
  lease_secs: number;
  paths?: string[];
  symbols?: string[];
}

export function taskLeaseState(task: LodestarTask, nowUnix: number): TaskLeaseState {
  if (task.status === "open") {
    return "claimable";
  }
  if (task.status === "claimed") {
    return typeof task.lease_expires_at === "number" && task.lease_expires_at >= nowUnix
      ? "live"
      : "expired";
  }
  if (task.status === "needs_input" || task.status === "paused") {
    return "parked";
  }
  return "unavailable";
}

export function canClaimTask(task: LodestarTask, nowUnix: number): boolean {
  const state = taskLeaseState(task, nowUnix);
  return (
    state === "claimable" || (state === "expired" && task.owner?.startsWith("session:v1:") === true)
  );
}

export function canRecoverLegacyClaim(task: LodestarTask, nowUnix: number): boolean {
  const owner = task.owner?.trim();
  if (!owner || owner.startsWith("session:v1:")) {
    return false;
  }
  if (task.status === "claimed") {
    return typeof task.lease_expires_at === "number" && task.lease_expires_at < nowUnix;
  }
  if (task.status === "needs_input" || task.status === "paused") {
    return typeof task.parked_at === "number" && task.parked_at + 7 * 24 * 3600 < nowUnix;
  }
  return false;
}

export function taskContextValue(
  task: LodestarTask,
  nowUnix: number,
  currentAgent?: string
): string {
  const state = taskLeaseState(task, nowUnix);
  if (task.status === "claimed" && state === "live") {
    return task.owner === currentAgent ? "claimed.owned" : "claimed";
  }
  const tags = [task.status];
  if (task.status === "paused" && task.owner === currentAgent) {
    tags.push("owned");
  }
  if (state === "claimable") {
    tags.push("claimable");
  } else if (state === "expired") {
    tags.push("expired", canRecoverLegacyClaim(task, nowUnix) ? "recoverable" : "claimable");
  } else if (state === "parked" && canRecoverLegacyClaim(task, nowUnix)) {
    tags.push("recoverable");
  }
  if (canRetireTask(task, nowUnix)) {
    tags.push("retireable");
  }
  return tags.join(".");
}

export function claimTaskRequest(
  task: LodestarTask,
  leaseSeconds: number,
  nowUnix: number,
  scope: TaskScope = { paths: [], symbols: [] }
): TaskLeaseRequest {
  if (!canClaimTask(task, nowUnix)) {
    throw new Error(`task ${task.id} is not claimable`);
  }
  return {
    ...leaseRequest(task.id, leaseSeconds),
    ...(scope.paths.length > 0 ? { paths: [...scope.paths] } : {}),
    ...(scope.symbols.length > 0 ? { symbols: [...scope.symbols] } : {}),
  };
}

export function parseTaskScope(paths: string, symbols: string): TaskScope {
  return {
    paths: scopeValues(paths, (value) => value.replace(/\\/g, "/")),
    symbols: scopeValues(symbols),
  };
}

export type OverlapSignal = "same_branch_collision" | "cross_branch_merge_risk" | "undeclared";

export interface OverlapPreflight {
  claims: Array<{
    task_id: string;
    owner: string;
    matching_paths?: string[];
    matching_symbols?: string[];
    owner_branch?: string | null;
    signal?: OverlapSignal;
  }>;
  footprints: Array<{
    agent_id: string;
    node_id: string;
    via_node_id?: string;
  }>;
}

/**
 * What the overlap actually costs, in the words a person deciding whether to
 * claim anyway needs (ADR-0035). An undeclared signal adds nothing: the reader
 * gets exactly the message they got before, because a guess dressed as a
 * warning is worse than the plain fact of the collision.
 */
function overlapSignalLabel(claim: {
  owner_branch?: string | null;
  signal?: OverlapSignal;
}): string {
  const branch = claim.owner_branch?.trim();
  switch (claim.signal) {
    case "same_branch_collision":
      return branch ? ` [same branch ${branch} \u2014 colliding now]` : "";
    case "cross_branch_merge_risk":
      return branch ? ` [on ${branch} \u2014 conflicts at merge]` : "";
    default:
      return "";
  }
}

export function overlapWarningDetail(preflight: OverlapPreflight): string | undefined {
  const lines = preflight.claims.slice(0, 5).map((claim) => {
    const matches = [...(claim.matching_paths ?? []), ...(claim.matching_symbols ?? [])];
    return (
      `Claim ${claim.task_id} (${claim.owner})${overlapSignalLabel(claim)}: ` +
      (matches.join(", ") || "matching scope")
    );
  });
  lines.push(
    ...preflight.footprints
      .slice(0, Math.max(0, 5 - lines.length))
      .map(
        (footprint) =>
          `Footprint ${footprint.agent_id}: ${footprint.node_id}` +
          (footprint.via_node_id ? ` via ${footprint.via_node_id}` : "")
      )
  );
  const hidden = preflight.claims.length + preflight.footprints.length - lines.length;
  if (hidden > 0) {
    lines.push(`...and ${hidden} more overlap${hidden === 1 ? "" : "s"}`);
  }
  return lines.length > 0 ? lines.join("\n") : undefined;
}

function scopeValues(input: string, normalize: (value: string) => string = (value) => value) {
  return [...new Set(input.split(/[,\r\n]+/).map((value) => normalize(value.trim())))].filter(
    Boolean
  );
}

export function renewTaskRequest(
  task: LodestarTask,
  leaseSeconds: number,
  nowUnix: number
): TaskLeaseRequest {
  if (taskLeaseState(task, nowUnix) !== "live" || !task.owner?.trim()) {
    throw new Error(`task ${task.id} does not have a renewable live claim`);
  }
  return leaseRequest(task.id, leaseSeconds);
}

export function releaseTaskRequest(
  task: LodestarTask,
  nowUnix: number
): Pick<TaskLeaseRequest, "task_id"> {
  if (taskLeaseState(task, nowUnix) !== "live" || !task.owner?.trim()) {
    throw new Error(`task ${task.id} does not have a releasable live claim`);
  }
  return { task_id: task.id };
}

function leaseRequest(taskId: string, leaseSeconds: number): TaskLeaseRequest {
  if (!Number.isInteger(leaseSeconds) || leaseSeconds < 60 || leaseSeconds > 8 * 3600) {
    throw new Error("lease duration must be a whole number from 60 to 28800 seconds");
  }
  return { task_id: taskId, lease_secs: leaseSeconds };
}

/** Whether a task can be deliberately retired without disturbing live ownership. */
export function canRetireTask(task: LodestarTask, nowUnix: number): boolean {
  switch (task.status) {
    case "open":
    case "in_review":
    case "blocked":
      return true;
    case "claimed":
      return typeof task.lease_expires_at === "number" && task.lease_expires_at < nowUnix;
    case "needs_input":
    case "paused":
    case "done":
    case "abandoned":
      return false;
    default:
      return false;
  }
}

export interface EvidenceRequest {
  task_id: string;
  agent_id: string;
  started_at: number;
  ended_at: number;
}

/** Build the MindLeak evidence request for one live Lodestar claim. */
export function evidenceRequestForTask(
  task: LodestarTask,
  fallbackAgent: string,
  endedAt: number
): EvidenceRequest {
  if (task.status !== "claimed") {
    throw new Error(`task ${task.id} is not claimed`);
  }
  const agent = task.owner?.trim() || fallbackAgent.trim();
  if (!agent) {
    throw new Error(`task ${task.id} has no agent identity`);
  }
  if (typeof task.claim_started_at !== "number") {
    throw new Error(`task ${task.id} has no claim start`);
  }
  if (endedAt < task.claim_started_at) {
    throw new Error(`task ${task.id} claim starts after the evidence window`);
  }
  return {
    task_id: task.id,
    agent_id: agent,
    started_at: task.claim_started_at,
    ended_at: endedAt,
  };
}

/**
 * The lease action a board task offers, if any: a `claimed` task can be
 * `pause`d; a `paused` task can be `resume`d. Any other state offers neither.
 * Pure so the portal can validate a possibly-stale board row before invoking
 * the owner-guarded lifecycle tool.
 */
export function leaseActionFor(
  task: LodestarTask,
  nowUnix = Math.floor(Date.now() / 1000)
): "pause" | "resume" | undefined {
  switch (task.status) {
    case "claimed":
      return taskLeaseState(task, nowUnix) === "live" ? "pause" : undefined;
    case "paused":
      return "resume";
    default:
      return undefined;
  }
}

/** A display row for the board tree. */
export interface BoardRow {
  id: string;
  label: string;
  description: string;
  tooltip: string;
  status: string;
  /** Codicon id for the row, derived from status *and* lease state. */
  icon: string;
}

const TASK_STATUS_ICONS: Record<string, string> = {
  claimed: "account",
  needs_input: "comment-unresolved",
  paused: "debug-pause",
  open: "circle-outline",
  in_review: "eye",
  blocked: "error",
  done: "check",
};

/**
 * The icon a board row should carry.
 *
 * A claim whose lease has expired is nobody's work: the store's compare-and-swap
 * admits it, and {@link boardRows} already sorts it with ready work. It still
 * drew the `account` icon, which is the icon for *someone is holding this* — so
 * the one row that means "abandoned, take it" looked identical to the rows that
 * mean "hands off". Fifteen such rows in a day made the board unreadable, and
 * the icon was the last part still saying the wrong thing.
 *
 * `watch` is the honest picture: a lease is a timer, and this one ran out.
 * Derived from the clock at render time, never reaped or written back.
 */
export function boardIconId(task: LodestarTask, nowUnix: number): string {
  if (task.status === "claimed" && taskLeaseState(task, nowUnix) === "expired") {
    return "watch";
  }
  return TASK_STATUS_ICONS[task.status] ?? "circle-slash";
}

const BOARD_STATUS_ORDER = [
  "needs_input",
  "in_review",
  "claimed",
  "paused",
  "open",
  "blocked",
  "done",
  "abandoned",
];
const TERMINAL_TASK_STATUSES = new Set(["done", "abandoned"]);
const TASK_STATUS_LABELS: Record<string, string> = {
  abandoned: "Retired",
  blocked: "Blocked",
  claimed: "In progress",
  done: "Verified",
  in_review: "Review needed",
  needs_input: "Input needed",
  open: "Ready",
  paused: "Paused",
};

function taskStatusLabel(status: string): string {
  return TASK_STATUS_LABELS[status] ?? status.replace(/_/g, " ");
}

/** Render the active board by default; terminal history remains explicitly available. */
export function boardRows(
  tasks: LodestarTask[],
  includeTerminal = false,
  nowUnix = Math.floor(Date.now() / 1000)
): BoardRow[] {
  const statusRank = (s: string): number => {
    const i = BOARD_STATUS_ORDER.indexOf(s);
    return i === -1 ? BOARD_STATUS_ORDER.length : i;
  };

  /**
   * Rank a row by what it means, not by the column it is stored in.
   *
   * A claim whose lease has expired is claimable by anyone — the store's
   * compare-and-swap admits `status = 'claimed' AND lease_expires_at < now`,
   * and the row already describes itself as "Claim expired · Ready". Sorting it
   * as `claimed` therefore puts abandoned work among work in progress. One
   * session left fifteen such rows behind in a day, which buried the three
   * tasks anybody was actually holding and made the board unreadable.
   *
   * Nothing is reaped or rewritten to achieve this: expiry is a function of
   * `lease_expires_at` and the clock, so it is derived at render time, the same
   * way effective edge weight is derived at query time.
   */
  const rank = (task: LodestarTask): [number, number] => {
    const state = taskLeaseState(task, nowUnix);
    const lapsed = task.status === "claimed" && state === "expired";
    // Genuinely untouched work sorts above work someone started and dropped.
    return [statusRank(lapsed ? "open" : task.status), lapsed ? 1 : 0];
  };

  return [...tasks]
    .filter((task) => includeTerminal || !TERMINAL_TASK_STATUSES.has(task.status))
    .sort((a, b) => {
      const [statusA, lapsedA] = rank(a);
      const [statusB, lapsedB] = rank(b);
      return statusA - statusB || lapsedA - lapsedB;
    })
    .map((t) => ({
      id: t.id,
      label: t.title,
      description: taskDescription(t, nowUnix),
      tooltip: taskTooltip(t, nowUnix),
      status: t.status,
      icon: boardIconId(t, nowUnix),
    }));
}

function taskDescription(task: LodestarTask, nowUnix: number): string {
  const state = taskLeaseState(task, nowUnix);
  const branch = task.branch?.trim();
  const owner = task.owner ?? "unknown";
  // Who holds it and the branch they hold it on, at the decision point: enough
  // for a colliding agent to tell a merge risk from the same work twice
  // (ADR-0035 d5). Omitted cleanly when no branch was declared.
  const who = branch ? `${owner} on ${branch}` : owner;
  let description: string;
  if (state === "expired") {
    description = `Claim expired · ${who} · Ready`;
  } else if (state === "live") {
    description = `In progress · ${who} · ${remainingLease(task, nowUnix)}`;
  } else if (state === "claimable") {
    description = "Ready";
  } else {
    const status = taskStatusLabel(task.status);
    description = task.owner ? `${status} · ${who}` : status;
  }
  const scopedItems = (task.scope?.paths.length ?? 0) + (task.scope?.symbols.length ?? 0);
  return scopedItems > 0 ? `${description} · ${scopedItems} scoped` : description;
}

function taskTooltip(task: LodestarTask, nowUnix: number): string {
  const lines = [task.title, `goal: ${task.goal_id}`, `status: ${taskStatusLabel(task.status)}`];
  if (task.owner) {
    lines.push(`owner: ${task.owner}`);
  }
  if (task.branch?.trim()) {
    lines.push(`branch: ${task.branch.trim()}`);
  }
  if (typeof task.claim_started_at === "number") {
    lines.push(`claim started: ${formatUnixSeconds(task.claim_started_at)}`);
  }
  if (typeof task.lease_expires_at === "number") {
    const state = taskLeaseState(task, nowUnix);
    lines.push(`lease expires: ${formatUnixSeconds(task.lease_expires_at)} (${state})`);
  }
  if (task.blocked_by) {
    lines.push(`blocked by: ${task.blocked_by}`);
  }
  if (task.scope?.paths.length) {
    lines.push(`scope paths: ${task.scope.paths.join(", ")}`);
  }
  if (task.scope?.symbols.length) {
    lines.push(`scope symbols: ${task.scope.symbols.join(", ")}`);
  }
  if (task.acceptance) {
    lines.push(task.acceptance);
  }
  return lines.join("\n") + formatGoverningClauses(task.governing);
}

function remainingLease(task: LodestarTask, nowUnix: number): string {
  const seconds = Math.max(0, (task.lease_expires_at ?? nowUnix) - nowUnix);
  if (seconds < 60) {
    return `${seconds}s left`;
  }
  return `${Math.ceil(seconds / 60)}m left`;
}

/** One entry in a task's durable thread (`task_query(view=thread)`). */
export interface TaskQaEntry {
  id: number;
  task_id: string;
  kind: string; // "question" | "answer"
  body: string;
  author: string;
  created_at: number;
}

/**
 * The pending question on a `needs_input` task: the body of the most recent
 * `question` entry in its Q&A thread, or undefined when there is none. Pure, so
 * it is unit-tested without the vscode API.
 */
export function pendingQuestion(thread: TaskQaEntry[]): string | undefined {
  if (!Array.isArray(thread)) {
    return undefined;
  }
  for (let i = thread.length - 1; i >= 0; i--) {
    if (thread[i]?.kind === "question") {
      return thread[i].body;
    }
  }
  return undefined;
}

/**
 * Render a task's durable Q&A thread (oldest first) as readable markdown. Pure
 * (no vscode API). Returns null when the thread is empty.
 */
export function formatQaThread(thread: TaskQaEntry[], taskTitle?: string): string | null {
  if (!Array.isArray(thread) || thread.length === 0) {
    return null;
  }
  const lines: string[] = [`# Q&A${taskTitle ? `: ${taskTitle}` : ""}`, ""];
  for (const entry of thread) {
    const who = entry.kind === "answer" ? `answer (${entry.author})` : `question (${entry.author})`;
    lines.push(`- **${who}** · ${formatUnixSeconds(entry.created_at)}`);
    lines.push(`  ${entry.body}`);
  }
  return lines.join("\n");
}

/** The result of the Lodestar `check_conformance` tool. */
export interface ConformanceResult {
  verdict: string;
  findings: string[];
}

export type DiagnosticSeverity = "error" | "warning" | "information";

export interface ConformanceDiagnostic {
  severity: DiagnosticSeverity;
  message: string;
}

/**
 * Map a conformance result to a diagnostic descriptor, or null when aligned (no
 * diagnostic). Pure — returns a plain object so it can be unit-tested without
 * the vscode API.
 */
export function conformanceDiagnostic(result: ConformanceResult): ConformanceDiagnostic | null {
  if (!result || result.verdict === "aligned") {
    return null;
  }
  const detail = result.findings?.length ? ` — ${result.findings.join("; ")}` : "";
  const severity: DiagnosticSeverity =
    result.verdict === "violation"
      ? "error"
      : result.verdict === "drift"
        ? "warning"
        : "information";
  return { severity, message: `MindLeak conformance: ${result.verdict}${detail}` };
}

/** One persisted conformance audit record from Lodestar `conformance_history`. */
export interface ConformanceRecord {
  id: number;
  task_id?: string | null;
  evidence_schema_version?: number;
  evidence: string;
  verdict: string;
  findings: string;
  checked_at: number;
}

/** The evidence bundle serialized (as JSON) inside a {@link ConformanceRecord}. */
export interface EvidenceBundle {
  summary?: string;
  changed_node_ids?: string[];
  failed_node_ids?: string[];
  execution_ids?: string[];
  commit_ids?: string[];
}

/**
 * Render a task's conformance audit chain (from `conformance_history`, oldest
 * first) as readable markdown: the most recent record in full — verdict,
 * findings, summary, and the changed/failed/execution/commit ids parsed from its
 * stored evidence bundle — plus any prior checks in time order. Pure (no vscode
 * API) so it is unit-tested directly. Returns null when no evidence is recorded.
 */
export function formatTaskEvidence(
  records: ConformanceRecord[],
  taskTitle?: string
): string | null {
  if (!Array.isArray(records) || records.length === 0) {
    return null;
  }
  const latest = records[records.length - 1];
  const bundle = parseEvidenceBundle(latest.evidence);
  const lines: string[] = [`# Conformance evidence${taskTitle ? `: ${taskTitle}` : ""}`, ""];
  lines.push(`- **Verdict:** ${latest.verdict}`);
  lines.push(`- **Checked:** ${formatUnixSeconds(latest.checked_at)}`);
  if (latest.findings) {
    lines.push(`- **Findings:** ${latest.findings}`);
  }
  if (bundle?.summary) {
    lines.push(`- **Summary:** ${bundle.summary}`);
  }
  appendIdList(lines, "Changed nodes", bundle?.changed_node_ids);
  appendIdList(lines, "Failed nodes", bundle?.failed_node_ids);
  appendIdList(lines, "Executions", bundle?.execution_ids);
  appendIdList(lines, "Commits", bundle?.commit_ids);
  if (records.length > 1) {
    lines.push("", "## Prior checks");
    for (const record of records.slice(0, -1)) {
      const detail = record.findings ? ` — ${record.findings}` : "";
      lines.push(`- ${formatUnixSeconds(record.checked_at)} — **${record.verdict}**${detail}`);
    }
  }
  return lines.join("\n");
}

function parseEvidenceBundle(raw: string): EvidenceBundle | null {
  if (!raw) {
    return null;
  }
  try {
    const parsed = JSON.parse(raw) as EvidenceBundle;
    return parsed && typeof parsed === "object" ? parsed : null;
  } catch {
    return null;
  }
}

function appendIdList(lines: string[], label: string, ids?: string[]): void {
  if (ids && ids.length) {
    lines.push(`- **${label}:** ${ids.join(", ")}`);
  }
}

function formatUnixSeconds(seconds: number): string {
  if (!Number.isFinite(seconds)) {
    return "unknown";
  }
  return `${new Date(seconds * 1000).toISOString().slice(0, 19).replace("T", " ")}Z`;
}

/** A task grouped with its conformance audit chain, for the Evidence Board. */
export interface EvidenceGroup {
  taskId: string;
  title: string;
  latestVerdict: string;
  checkCount: number;
  records: ConformanceRecord[];
}

/**
 * Group tasks that have a conformance audit chain into Evidence Board rows,
 * most-recently-checked first — the freshest proof leads. Tasks with no records
 * are omitted (nothing to show). Pure: the vscode tree just renders these.
 */
export function evidenceGroups(
  tasks: LodestarTask[],
  historyByTask: Record<string, ConformanceRecord[]>
): EvidenceGroup[] {
  const groups: EvidenceGroup[] = [];
  for (const task of tasks ?? []) {
    const records = historyByTask?.[task.id];
    if (!Array.isArray(records) || records.length === 0) {
      continue;
    }
    const latest = records[records.length - 1];
    groups.push({
      taskId: task.id,
      title: task.title,
      latestVerdict: latest.verdict,
      checkCount: records.length,
      records,
    });
  }
  groups.sort((a, b) => lastCheckedAt(b.records) - lastCheckedAt(a.records));
  return groups;
}

function lastCheckedAt(records: ConformanceRecord[]): number {
  return records.length ? (records[records.length - 1].checked_at ?? 0) : 0;
}

/** A ThemeIcon id for a conformance verdict. Pure — no vscode import. */
export function verdictIconId(verdict: string): string {
  switch (verdict) {
    case "aligned":
      return "verified-filled";
    case "drift":
      return "warning";
    case "violation":
      return "error";
    case "needs_human":
      return "person";
    default:
      return "question";
  }
}

// ---- Telemetry & effectiveness (real-time observability pane) ---------------

export function shouldPollTelemetry(visible: boolean, live: boolean): boolean {
  return visible && live;
}

/**
 * Aggregate metrics for one tool, as returned by `telemetry_snapshot`.
 *
 * `calls`/`errors` are lifetime totals over the append-only trail — they never
 * shrink. Current health is the separate, recency-based `currently_failing`
 * (the tool's most recent call errored); `last_error_at`/`last_error_detail`
 * keep a resolved historical failure queryable without presenting it as live.
 */
export interface TelemetryToolMetric {
  name: string;
  calls: number;
  errors: number;
  total_ms: number;
  min_ms: number;
  max_ms: number;
  avg_ms: number;
  last_success_at?: number | null;
  last_error_at?: number | null;
  last_error_detail?: unknown;
  last_degraded_at?: number | null;
  last_degraded_detail?: unknown;
  currently_failing?: boolean;
  currently_degraded?: boolean;
}

/** One recorded event from `telemetry_snapshot.recent`. */
export interface TelemetryEvent {
  ts: number;
  kind: string;
  name: string;
  outcome: string;
  duration_ms?: number | null;
  detail?: unknown;
}

/** Deterministic call-volume and memory-habit interpretation from the server. */
export interface UsageRetrospective {
  background_read_calls: number;
  preflight_read_calls: number;
  architectural_decision_calls: number;
  writing_sessions: number;
  writing_sessions_without_memory_read: number;
  recommendations: string[];
}

/** The `telemetry_snapshot` tool result (subset used by the pane). */
export interface TelemetrySnapshot {
  total_events: number;
  total_errors: number;
  /** How many tools are failing right now (most recent call errored). */
  currently_failing_tools?: number;
  /** How many tools are degraded right now (most recent call skipped). */
  currently_degraded_tools?: number;
  by_name: TelemetryToolMetric[];
  recent: TelemetryEvent[];
  /** Current graph health bundled by telemetry_snapshot to avoid a second poll. */
  graph?: GraphCounts;
  retrospective?: UsageRetrospective;
}

/** The `graph_stats` tool result. */
export interface GraphCounts {
  nodes: number;
  active_edges: number;
}

/** A per-tool effectiveness row rendered in the telemetry pane. */
export interface TelemetryToolRow {
  name: string;
  calls: number;
  errors: number;
  errorRatePct: number;
  avgMs: number;
  /** The tool's most recent call errored — a live fault, not lifetime history. */
  currentlyFailing: boolean;
  /** The tool's most recent call skipped optional work. */
  currentlyDegraded: boolean;
}

/** The derived, real-time effectiveness readout for the telemetry pane. */
export interface TelemetryDashboard {
  nodes: number;
  activeEdges: number;
  totalEvents: number;
  /** Lifetime error count — cumulative history, not the current fault state. */
  totalErrors: number;
  /** Tools failing right now (most recent call errored) — the live health signal. */
  failingTools: number;
  /** Tools degraded right now without a deterministic-path failure. */
  degradedTools: number;
  successRatePct: number;
  errorRatePct: number;
  avgLatencyMs: number;
  backgroundReadCalls: number;
  preflightReadCalls: number;
  memoryPreflightMisses: number;
  recommendations: string[];
  tools: TelemetryToolRow[];
}

function round1(value: number): number {
  return Math.round(value * 10) / 10;
}

/**
 * Compute the real-time effectiveness readout from a telemetry snapshot and the
 * current graph counts. Pure so the pane's numbers are unit-tested without the
 * webview. Effectiveness = how reliably and quickly the engine has served tool
 * calls, alongside how much live context it currently holds.
 */
export function telemetryDashboard(
  snapshot: TelemetrySnapshot | undefined,
  counts?: GraphCounts
): TelemetryDashboard {
  const graph = counts ?? snapshot?.graph;
  const totalEvents = snapshot?.total_events ?? 0;
  const totalErrors = snapshot?.total_errors ?? 0;
  const byName = snapshot?.by_name ?? [];
  const successRatePct =
    totalEvents === 0 ? 100 : ((totalEvents - totalErrors) / totalEvents) * 100;
  const totalMs = byName.reduce((sum, tool) => sum + tool.total_ms, 0);
  const totalCalls = byName.reduce((sum, tool) => sum + tool.calls, 0);
  const avgLatencyMs = totalCalls === 0 ? 0 : totalMs / totalCalls;
  const tools = [...byName]
    .sort((a, b) => b.calls - a.calls || a.name.localeCompare(b.name))
    .map((tool) => ({
      name: tool.name,
      calls: tool.calls,
      errors: tool.errors,
      errorRatePct: tool.calls === 0 ? 0 : round1((tool.errors / tool.calls) * 100),
      avgMs: round1(tool.avg_ms),
      currentlyFailing: tool.currently_failing === true,
      currentlyDegraded: tool.currently_degraded === true,
    }));
  const failingTools =
    snapshot?.currently_failing_tools ?? tools.filter((tool) => tool.currentlyFailing).length;
  const degradedTools =
    snapshot?.currently_degraded_tools ?? tools.filter((tool) => tool.currentlyDegraded).length;
  const retrospective = snapshot?.retrospective;
  return {
    nodes: graph?.nodes ?? 0,
    activeEdges: graph?.active_edges ?? 0,
    totalEvents,
    totalErrors,
    failingTools,
    degradedTools,
    successRatePct: round1(successRatePct),
    errorRatePct: round1(100 - successRatePct),
    avgLatencyMs: round1(avgLatencyMs),
    backgroundReadCalls: retrospective?.background_read_calls ?? 0,
    preflightReadCalls: retrospective?.preflight_read_calls ?? 0,
    memoryPreflightMisses: retrospective?.writing_sessions_without_memory_read ?? 0,
    recommendations: retrospective?.recommendations ?? [],
    tools,
  };
}

/**
 * Format one telemetry event as a single live-log line. UTC time keeps the
 * output deterministic and stable across machines.
 */
export function formatLogLine(event: TelemetryEvent): string {
  const time = new Date(event.ts * 1000).toISOString().slice(11, 19);
  const duration = typeof event.duration_ms === "number" ? ` ${event.duration_ms}ms` : "";
  return `${time} ${event.outcome} ${event.kind}:${event.name}${duration}`;
}

/**
 * Build the live-log lines (newest first) from a snapshot, capped. Returns an
 * empty list when live logging is off so the pane never renders a stream the
 * user has disabled.
 */
export function logLines(
  snapshot: TelemetrySnapshot | undefined,
  live: boolean,
  max = 200
): string[] {
  if (!live || !snapshot?.recent?.length) {
    return [];
  }
  return snapshot.recent.slice(0, Math.max(0, max)).map(formatLogLine);
}
