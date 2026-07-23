// Pure, dependency-free helpers (no vscode / fs imports) so they can be unit-tested.
import * as path from "path";

/** Convert a workspace-relative path to a MindLeak artifact node id. */
export function toArtifactId(relPath: string): string {
  return `artifact:${relPath.replace(/\\/g, "/")}`;
}

/**
 * Parse an MCP tool result's first text-content block as JSON. Falls back to the
 * raw text (or the whole result) when it is not JSON.
 */
export function parseToolResult(result: unknown): unknown {
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

/** A display row for the board tree. */
export interface BoardRow {
  id: string;
  label: string;
  description: string;
  tooltip: string;
  status: string;
}

const BOARD_STATUS_ORDER = ["claimed", "open", "in_review", "blocked", "done", "abandoned"];
const TERMINAL_TASK_STATUSES = new Set(["done", "abandoned"]);

/** Render the active board by default; terminal history remains explicitly available. */
export function boardRows(tasks: LodestarTask[], includeTerminal = false): BoardRow[] {
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
      description: t.owner ? `${t.status} · ${t.owner}` : t.status,
      tooltip: `${t.title}\ngoal: ${t.goal_id}${t.acceptance ? `\n${t.acceptance}` : ""}`,
      status: t.status,
    }));
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

/** Aggregate metrics for one tool, as returned by `telemetry_snapshot`. */
export interface TelemetryToolMetric {
  name: string;
  calls: number;
  errors: number;
  total_ms: number;
  min_ms: number;
  max_ms: number;
  avg_ms: number;
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
}

/** The derived, real-time effectiveness readout for the telemetry pane. */
export interface TelemetryDashboard {
  nodes: number;
  activeEdges: number;
  totalEvents: number;
  totalErrors: number;
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
    }));
  return {
    nodes: counts?.nodes ?? 0,
    activeEdges: counts?.active_edges ?? 0,
    totalEvents,
    totalErrors,
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
