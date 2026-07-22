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

export interface ResolveServerOptions {
  platform?: NodeJS.Platform;
  exists?: (candidate: string) => boolean;
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
 * Prefer a workspace-built binary when the configured path is the bare default
 * name. Generic over the binary (`mindleak-mcp` / `lodestar-mcp`); `exists` and
 * `platform` are injectable so this stays pure and testable.
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

/** Order tasks by lifecycle and render display fields. Pure and testable. */
export function boardRows(tasks: LodestarTask[]): BoardRow[] {
  const rank = (s: string): number => {
    const i = BOARD_STATUS_ORDER.indexOf(s);
    return i === -1 ? BOARD_STATUS_ORDER.length : i;
  };
  return [...tasks]
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
