// Pure, dependency-free helpers (no vscode / fs imports) so they can be unit-tested.
import * as path from "path";

/** Convert a workspace-relative path to a MindLeak artifact node id. */
export function toArtifactId(relPath: string): string {
  return `artifact:${relPath.replace(/\\/g, "/")}`;
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
  extensionPath?: string;
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
 * Prefer the packaged binary, then a workspace build, when the configured path
 * is the bare default name. Generic over both MCP server binaries; filesystem
 * inputs are injectable so this stays pure and testable.
 */
export function resolveBinaryPath(
  configured: string,
  workspace: string,
  binaryName: string,
  opts: ResolveServerOptions = {}
): string {
  const platform = opts.platform ?? process.platform;
  const exists = opts.exists ?? (() => false);
  if (configured && configured !== binaryName) {
    return configured;
  }
  const exe = platform === "win32" ? `${binaryName}.exe` : binaryName;
  if (opts.extensionPath) {
    const packaged = path.join(opts.extensionPath, "bin", exe);
    if (exists(packaged)) {
      return packaged;
    }
  }
  for (const profile of ["release", "debug"]) {
    const candidate = path.join(workspace, "target", profile, exe);
    if (exists(candidate)) {
      return candidate;
    }
  }
  return configured || binaryName;
}

/** A task as returned by the Lodestar `board` tool (subset used by the UI). */
export interface LodestarTask {
  id: string;
  goal_id: string;
  title: string;
  acceptance?: string;
  status: string;
  owner?: string | null;
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
  agent: string;
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
  return state === "claimable" || state === "expired";
}

export function taskContextValue(task: LodestarTask, nowUnix: number): string {
  const state = taskLeaseState(task, nowUnix);
  if (task.status === "claimed" && state === "live") {
    return "claimed";
  }
  const tags = [task.status];
  if (state === "claimable") {
    tags.push("claimable");
  } else if (state === "expired") {
    tags.push("expired", "claimable");
  }
  if (canRetireTask(task, nowUnix)) {
    tags.push("retireable");
  }
  return tags.join(".");
}

export function claimTaskRequest(
  task: LodestarTask,
  agent: string,
  leaseSeconds: number,
  nowUnix: number,
  scope: TaskScope = { paths: [], symbols: [] }
): TaskLeaseRequest {
  if (!canClaimTask(task, nowUnix)) {
    throw new Error(`task ${task.id} is not claimable`);
  }
  return {
    ...leaseRequest(task.id, agent, leaseSeconds),
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

export interface OverlapPreflight {
  claims: Array<{
    task_id: string;
    owner: string;
    matching_paths?: string[];
    matching_symbols?: string[];
  }>;
  footprints: Array<{
    agent_id: string;
    node_id: string;
    via_node_id?: string;
  }>;
}

export function overlapWarningDetail(preflight: OverlapPreflight): string | undefined {
  const lines = preflight.claims.slice(0, 5).map((claim) => {
    const matches = [...(claim.matching_paths ?? []), ...(claim.matching_symbols ?? [])];
    return `Claim ${claim.task_id} (${claim.owner}): ${matches.join(", ") || "matching scope"}`;
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
  return leaseRequest(task.id, task.owner, leaseSeconds);
}

export function releaseTaskRequest(
  task: LodestarTask,
  nowUnix: number
): Pick<TaskLeaseRequest, "task_id" | "agent"> {
  if (taskLeaseState(task, nowUnix) !== "live" || !task.owner?.trim()) {
    throw new Error(`task ${task.id} does not have a releasable live claim`);
  }
  return { task_id: task.id, agent: task.owner.trim() };
}

function leaseRequest(taskId: string, agent: string, leaseSeconds: number): TaskLeaseRequest {
  const identity = agent.trim();
  if (!identity) {
    throw new Error("an agent identity is required");
  }
  if (!Number.isInteger(leaseSeconds) || leaseSeconds < 60 || leaseSeconds > 8 * 3600) {
    throw new Error("lease duration must be a whole number from 60 to 28800 seconds");
  }
  return { task_id: taskId, agent: identity, lease_secs: leaseSeconds };
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
}

const BOARD_STATUS_ORDER = [
  "needs_input",
  "claimed",
  "paused",
  "open",
  "in_review",
  "blocked",
  "done",
  "abandoned",
];
const TERMINAL_TASK_STATUSES = new Set(["done", "abandoned"]);

/** Render the active board by default; terminal history remains explicitly available. */
export function boardRows(
  tasks: LodestarTask[],
  includeTerminal = false,
  nowUnix = Math.floor(Date.now() / 1000)
): BoardRow[] {
  const rank = (s: string): number => {
    const i = BOARD_STATUS_ORDER.indexOf(s);
    return i === -1 ? BOARD_STATUS_ORDER.length : i;
  };
  return [...tasks]
    .filter((task) => includeTerminal || !TERMINAL_TASK_STATUSES.has(task.status))
    .sort((a, b) => rank(a.status) - rank(b.status))
    .map((t) => ({
      id: t.id,
      label: t.title,
      description: taskDescription(t, nowUnix),
      tooltip: taskTooltip(t, nowUnix),
      status: t.status,
    }));
}

function taskDescription(task: LodestarTask, nowUnix: number): string {
  const state = taskLeaseState(task, nowUnix);
  let description: string;
  if (state === "expired") {
    description = `expired claim · ${task.owner ?? "unknown"} · reclaimable`;
  } else if (state === "live") {
    description = `claimed · ${task.owner ?? "unknown"} · ${remainingLease(task, nowUnix)}`;
  } else if (state === "claimable") {
    description = "open · claimable";
  } else {
    description = task.owner ? `${task.status} · ${task.owner}` : task.status;
  }
  const scopedItems = (task.scope?.paths.length ?? 0) + (task.scope?.symbols.length ?? 0);
  return scopedItems > 0 ? `${description} · ${scopedItems} scoped` : description;
}

function taskTooltip(task: LodestarTask, nowUnix: number): string {
  const lines = [task.title, `goal: ${task.goal_id}`, `status: ${task.status}`];
  if (task.owner) {
    lines.push(`owner: ${task.owner}`);
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

/** One entry in a task's durable question/answer thread (Lodestar `task_qa`). */
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

// ---- Telemetry & effectiveness (real-time observability pane) ---------------

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
  currently_failing?: boolean;
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

/** The `telemetry_snapshot` tool result (subset used by the pane). */
export interface TelemetrySnapshot {
  total_events: number;
  total_errors: number;
  /** How many tools are failing right now (most recent call errored). */
  currently_failing_tools?: number;
  by_name: TelemetryToolMetric[];
  recent: TelemetryEvent[];
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
  successRatePct: number;
  errorRatePct: number;
  avgLatencyMs: number;
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
  counts: GraphCounts | undefined
): TelemetryDashboard {
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
    }));
  const failingTools =
    snapshot?.currently_failing_tools ?? tools.filter((tool) => tool.currentlyFailing).length;
  return {
    nodes: counts?.nodes ?? 0,
    activeEdges: counts?.active_edges ?? 0,
    totalEvents,
    totalErrors,
    failingTools,
    successRatePct: round1(successRatePct),
    errorRatePct: round1(100 - successRatePct),
    avgLatencyMs: round1(avgLatencyMs),
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
