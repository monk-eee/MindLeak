import {
  canClaimTask,
  canRecoverLegacyClaim,
  leaseActionFor,
  LodestarTask,
  taskLeaseState,
  TaskLeaseState,
} from "./util";

/**
 * How recently a session must have spoken to count as part of the live fleet.
 *
 * The ledger keeps every session that ever opened — 66 sessions and 127 agents
 * on this repository — so an unfiltered roster is a history, not a fleet. A
 * session holding a task is always live regardless of this window: work in
 * flight is the whole question the pane exists to answer.
 */
export const DEFAULT_ACTIVE_WINDOW_SECONDS = 30 * 60;

/** Self-declared working context from `open_session` (ADR-0035, ADR-0044). */
export interface FleetSessionContext {
  base?: string | null;
  behind?: number | null;
  branch?: string | null;
  dirty?: boolean | null;
  head_sha?: string | null;
}

/** One session as Lodestar `fleet_view` reports it. */
export interface FleetSession {
  agent_id: string;
  claimed_task_ids?: string[] | null;
  context?: FleetSessionContext | null;
  declared_at?: number | null;
  staleness?: { commits?: number | null; state?: string | null } | null;
}

/** The `fleet_view` payload (subset the pane renders). */
export interface FleetSnapshot {
  sessions?: FleetSession[] | null;
  divergence?: {
    bases?: string[] | null;
    diverged?: boolean | null;
    undeclared_sessions?: number | null;
  } | null;
  enforcement?: string | null;
}

/** One agent as MindLeak `list_agents` reports it. */
export interface AgentActivity {
  id: string;
  label?: string | null;
  last_active?: number | null;
  observations?: number | null;
}

/** The `list_agents` payload. */
export interface AgentRoster {
  agents?: AgentActivity[] | null;
}

/** One entry from `task_query(view=stalled)`. */
export interface StalledEntry {
  task_id: string;
  title?: string | null;
  kind?: string | null;
  since?: number | null;
  stalled_seconds?: number | null;
  detail?: string | null;
}

/**
 * One finding from `task_query(view=doctor)`. `subject` is what the finding is
 * about — a repeated title, or the blocked task's own title — not a free-form
 * `detail`: the wire field is `subject` (`crates/lodestar-core/src/model/executive.rs`
 * `BoardFinding`), and nothing here ever read the old `detail` name.
 */
export interface BoardFinding {
  ailment?: string | null;
  subject?: string | null;
  remedy?: string | null;
  task_ids?: string[] | null;
}

/** A stable, worst-first order for grouping doctor findings by ailment. */
const AILMENT_ORDER = ["blocked_without_gate", "same_title_across_goals", "duplicate_title"];

/** A short, human label for one `BoardFinding.ailment`. */
export function doctorAilmentLabel(ailment: string): string {
  switch (ailment) {
    case "duplicate_title":
      return "Duplicate title";
    case "same_title_across_goals":
      return "Forked across goals";
    case "blocked_without_gate":
      return "Blocked with no gate";
    default:
      return ailment;
  }
}

/** A `ThemeIcon` id for one `BoardFinding.ailment`. Pure — no vscode import. */
export function doctorAilmentIcon(ailment: string): string {
  switch (ailment) {
    case "blocked_without_gate":
      return "circle-slash";
    case "same_title_across_goals":
      return "type-hierarchy";
    case "duplicate_title":
      return "copy";
    default:
      return "question";
  }
}

/** One ailment's findings, grouped for the Board Doctor pane. */
export interface DoctorGroup {
  ailment: string;
  label: string;
  findings: BoardFinding[];
}

/**
 * Group `task_query(view=doctor)` findings by ailment, worst-first
 * ({@link AILMENT_ORDER}), an unrecognised ailment sorting last. Pure: the
 * tree just renders these. Empty ailments are omitted — an empty group is not
 * a finding.
 */
export function doctorGroups(findings: BoardFinding[]): DoctorGroup[] {
  const byAilment = new Map<string, BoardFinding[]>();
  for (const finding of findings ?? []) {
    const ailment = finding.ailment ?? "unknown";
    const group = byAilment.get(ailment);
    if (group) {
      group.push(finding);
    } else {
      byAilment.set(ailment, [finding]);
    }
  }
  const rank = (ailment: string) => {
    const index = AILMENT_ORDER.indexOf(ailment);
    return index === -1 ? AILMENT_ORDER.length : index;
  };
  return [...byAilment.entries()]
    .sort(([a], [b]) => rank(a) - rank(b))
    .map(([ailment, groupFindings]) => ({
      ailment,
      label: doctorAilmentLabel(ailment),
      findings: groupFindings,
    }));
}

/**
 * A lifecycle verb the pane may offer for a row.
 *
 * Every one already exists as a Lodestar tool; the pane adds no verb and
 * decides no policy. `recover` is deliberately *not* the answer to an ordinary
 * lapsed lease — that is an ordinary `claim`. `recover` is for a stranded
 * legacy identity, which is what {@link canRecoverLegacyClaim} tests.
 */
export type FleetVerb = "renew" | "release" | "pause" | "resume" | "claim" | "recover";

/** A task rendered under the agent that holds it. */
export interface FleetTaskRow {
  id: string;
  title: string;
  status: string;
  lease: TaskLeaseState;
  /** Seconds until the lease expires; negative once it has lapsed. */
  leaseSeconds: number | null;
  verbs: FleetVerb[];
}

/** What an agent is doing, as far as anything can honestly say. */
export type FleetRowState = "holding" | "lapsed" | "idle";

/** One agent row. Absent declarations stay `null` and render as unknown. */
export interface FleetRow {
  agentId: string;
  /** Last 12 characters of the fingerprint — enough to tell agents apart. */
  short: string;
  isSelf: boolean;
  branch: string | null;
  head: string | null;
  base: string | null;
  behind: number | null;
  dirty: boolean | null;
  staleness: string | null;
  lastActive: number | null;
  observations: number | null;
  tasks: FleetTaskRow[];
  state: FleetRowState;
}

/** Whether each plane answered this refresh. */
export interface PlaneHealth {
  intent: boolean;
  memory: boolean;
}

/** Everything the pane renders in one refresh. */
export interface FleetDashboard {
  generatedAt: number;
  planes: PlaneHealth;
  rows: FleetRow[];
  /** Live sessions suppressed by the activity window. */
  hidden: number;
  divergence: {
    bases: string[];
    diverged: boolean;
    undeclaredSessions: number;
  };
  stalled: StalledEntry[];
  ailments: BoardFinding[];
  /** Set when the readout is degraded, so the pane never implies completeness. */
  notice: string | null;
}

export interface FleetInput {
  snapshot?: FleetSnapshot | null;
  roster?: AgentRoster | null;
  tasks?: LodestarTask[] | null;
  stalled?: StalledEntry[] | null;
  ailments?: BoardFinding[] | null;
  planes: PlaneHealth;
  selfAgentId?: string | null;
  nowUnix: number;
  activeWindowSeconds?: number;
}

/** Shorten an opaque agent id for display without pretending it is a name. */
export function shortAgent(agentId: string): string {
  const trimmed = agentId.trim();
  const fingerprint = trimmed.split(":").pop() ?? trimmed;
  return fingerprint.length > 12 ? fingerprint.slice(-12) : fingerprint;
}

/**
 * The verbs legal for one task from this session's point of view.
 *
 * Composed from the existing guards rather than restating them, so the pane
 * cannot drift from what the server will actually accept. An action that would
 * be refused is not offered.
 */
export function verbsForTask(task: LodestarTask, isSelf: boolean, nowUnix: number): FleetVerb[] {
  const verbs: FleetVerb[] = [];
  const lease = taskLeaseState(task, nowUnix);
  const action = leaseActionFor(task, nowUnix);

  if (isSelf) {
    if (task.status === "claimed" && lease === "live") {
      verbs.push("renew", "release");
    }
    if (action === "pause") {
      verbs.push("pause");
    }
    if (action === "resume") {
      verbs.push("resume");
    }
    return verbs;
  }

  // A lapsed session claim is ordinary claimable work; only a stranded legacy
  // identity needs the attributed recovery path.
  if (canRecoverLegacyClaim(task, nowUnix)) {
    verbs.push("recover");
  } else if (canClaimTask(task, nowUnix)) {
    verbs.push("claim");
  }
  return verbs;
}

function normaliseContext(context: FleetSessionContext | null | undefined) {
  const value = context ?? {};
  const text = (raw: unknown): string | null => {
    if (typeof raw !== "string") {
      return null;
    }
    const trimmed = raw.trim();
    return trimmed.length > 0 ? trimmed : null;
  };
  return {
    branch: text(value.branch),
    head: text(value.head_sha),
    base: text(value.base),
    behind: typeof value.behind === "number" ? value.behind : null,
    dirty: typeof value.dirty === "boolean" ? value.dirty : null,
  };
}

function rowState(tasks: FleetTaskRow[]): FleetRowState {
  if (tasks.length === 0) {
    return "idle";
  }
  return tasks.some((task) => task.lease === "expired") ? "lapsed" : "holding";
}

/**
 * Build the fleet readout.
 *
 * Sessions and agents are joined on the opaque agent id, which both planes
 * share (ADR-0054), so a row can carry Lodestar's declared context and
 * MindLeak's observed activity at once. Every value is self-reported: an
 * undeclared field stays `null` here and is rendered as unknown rather than
 * guessed.
 */
export function fleetDashboard(input: FleetInput): FleetDashboard {
  const {
    snapshot,
    roster,
    tasks,
    stalled,
    ailments,
    planes,
    selfAgentId,
    nowUnix,
    activeWindowSeconds = DEFAULT_ACTIVE_WINDOW_SECONDS,
  } = input;

  const sessions = Array.isArray(snapshot?.sessions) ? snapshot!.sessions! : [];
  const agents = Array.isArray(roster?.agents) ? roster!.agents! : [];
  const board = Array.isArray(tasks) ? tasks : [];

  const activity = new Map<string, AgentActivity>();
  for (const agent of agents) {
    if (typeof agent?.id !== "string") {
      continue;
    }
    // MindLeak keys attribution nodes as `agent:<session id>`.
    activity.set(agent.id.replace(/^agent:/, ""), agent);
  }

  const tasksByOwner = new Map<string, LodestarTask[]>();
  for (const task of board) {
    const owner = task.owner?.trim();
    if (!owner) {
      continue;
    }
    const held = tasksByOwner.get(owner) ?? [];
    held.push(task);
    tasksByOwner.set(owner, held);
  }

  let hidden = 0;
  const rows: FleetRow[] = [];
  for (const session of sessions) {
    if (typeof session?.agent_id !== "string" || !session.agent_id.trim()) {
      continue;
    }
    const agentId = session.agent_id.trim();
    const isSelf = Boolean(selfAgentId && agentId === selfAgentId);
    const held = tasksByOwner.get(agentId) ?? [];
    const observed = activity.get(agentId);

    const lastActive =
      typeof observed?.last_active === "number"
        ? observed.last_active
        : typeof session.declared_at === "number"
          ? session.declared_at
          : null;

    const recent = lastActive !== null && nowUnix - lastActive <= activeWindowSeconds;
    if (!isSelf && held.length === 0 && !recent) {
      hidden += 1;
      continue;
    }

    const context = normaliseContext(session.context);
    const taskRows: FleetTaskRow[] = held.map((task) => ({
      id: task.id,
      title: task.title,
      status: task.status,
      lease: taskLeaseState(task, nowUnix),
      leaseSeconds:
        typeof task.lease_expires_at === "number" ? task.lease_expires_at - nowUnix : null,
      verbs: verbsForTask(task, isSelf, nowUnix),
    }));

    rows.push({
      agentId,
      short: shortAgent(agentId),
      isSelf,
      branch: context.branch,
      head: context.head,
      base: context.base,
      behind: context.behind,
      dirty: context.dirty,
      staleness: typeof session.staleness?.state === "string" ? session.staleness.state : null,
      lastActive,
      observations: typeof observed?.observations === "number" ? observed.observations : null,
      tasks: taskRows,
      state: rowState(taskRows),
    });
  }

  // Self first, then agents in trouble, then the most recently active.
  const rank = (row: FleetRow): number => (row.isSelf ? 0 : row.state === "lapsed" ? 1 : 2);
  rows.sort((a, b) => {
    const byRank = rank(a) - rank(b);
    if (byRank !== 0) {
      return byRank;
    }
    return (b.lastActive ?? 0) - (a.lastActive ?? 0);
  });

  const notices: string[] = [];
  if (!planes.intent) {
    notices.push("Lodestar is not answering, so no roster or claim is shown.");
  }
  if (!planes.memory) {
    notices.push("MindLeak is not answering, so observed activity is missing.");
  }

  return {
    generatedAt: nowUnix,
    planes,
    rows: planes.intent ? rows : [],
    hidden: planes.intent ? hidden : 0,
    divergence: {
      bases: Array.isArray(snapshot?.divergence?.bases) ? snapshot!.divergence!.bases! : [],
      diverged: snapshot?.divergence?.diverged === true,
      undeclaredSessions:
        typeof snapshot?.divergence?.undeclared_sessions === "number"
          ? snapshot!.divergence!.undeclared_sessions!
          : 0,
    },
    stalled: Array.isArray(stalled) ? stalled : [],
    ailments: Array.isArray(ailments) ? ailments : [],
    notice: notices.length > 0 ? notices.join(" ") : null,
  };
}

/** Render a duration as a compact human span; `null` becomes an explicit unknown. */
export function formatDuration(seconds: number | null): string {
  if (seconds === null || !Number.isFinite(seconds)) {
    return "unknown";
  }
  const magnitude = Math.abs(Math.floor(seconds));
  if (magnitude < 60) {
    return `${magnitude}s`;
  }
  if (magnitude < 3600) {
    return `${Math.floor(magnitude / 60)}m`;
  }
  if (magnitude < 86400) {
    return `${Math.floor(magnitude / 3600)}h`;
  }
  return `${Math.floor(magnitude / 86400)}d`;
}
