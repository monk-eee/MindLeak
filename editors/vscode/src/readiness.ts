export type ReadinessState =
  "disconnected" | "ready_empty" | "observing" | "coordinating" | "degraded_optional";

export interface ServerIdentity {
  name: string;
  version: string;
}

export interface ReadinessInput {
  memoryConnected: boolean;
  intentConnected: boolean;
  memoryServer?: ServerIdentity;
  intentServer?: ServerIdentity;
  memoryError?: string;
  intentError?: string;
  graph?: {
    nodes: number;
    active_edges: number;
  };
  actionableTasks: number;
  actionableDesigns: number;
  hasActiveFile: boolean;
  terminalHealth: string;
  gitHealth: string;
  agentId: string;
}

export interface ReadinessAction {
  label: string;
  command: string;
  arguments?: unknown[];
}

export interface ReadinessSnapshot {
  state: ReadinessState;
  title: string;
  detail: string;
  action: ReadinessAction;
  memory: string;
  intent: string;
  agent: string;
  degradedCapabilities: string[];
  graph: {
    nodes: number;
    activeEdges: number;
  };
  actionableTasks: number;
  actionableDesigns: number;
}

export type ReadinessRowKind = "state" | "server" | "agent" | "optional";

export interface ReadinessRow {
  id: string;
  kind: ReadinessRowKind;
  label: string;
  description: string;
  tooltip: string;
  icon: string;
  action?: ReadinessAction;
}

export function sessionAgentIdentity(base: string, nonce: string): string {
  const label = base.trim() || "vscode";
  const suffix = nonce
    .toLowerCase()
    .replace(/[^a-z0-9]/g, "")
    .slice(0, 8);
  return suffix ? `${label}-${suffix}` : label;
}

export function deriveReadiness(input: ReadinessInput): ReadinessSnapshot {
  const degradedCapabilities = optionalDegradations(input);
  const memory = serverLabel("Memory", input.memoryConnected, input.memoryServer);
  const intent = serverLabel("Intent", input.intentConnected, input.intentServer);
  const graph = {
    nodes: input.graph?.nodes ?? 0,
    activeEdges: input.graph?.active_edges ?? 0,
  };
  const common = {
    memory,
    intent,
    agent: input.agentId || "not configured",
    degradedCapabilities,
    graph,
    actionableTasks: input.actionableTasks,
    actionableDesigns: input.actionableDesigns,
  };

  if (!input.memoryConnected || !input.intentConnected || input.memoryError || input.intentError) {
    const failures = [
      !input.memoryConnected || input.memoryError
        ? `Memory: ${input.memoryError ?? "server unavailable"}`
        : undefined,
      !input.intentConnected || input.intentError
        ? `Intent: ${input.intentError ?? "server unavailable"}`
        : undefined,
    ].filter((value): value is string => Boolean(value));
    return {
      ...common,
      state: "disconnected",
      title: "Connection needs attention",
      detail: failures.join("; "),
      action: {
        label: "Open MindLeak settings",
        command: "workbench.action.openSettings",
        arguments: ["@ext:monk-eee.mindleak"],
      },
    };
  }

  if (graph.nodes === 0) {
    return {
      ...common,
      state: "ready_empty",
      title: "Ready for first context",
      detail: input.hasActiveFile
        ? "Both planes are connected. Ingest the active file to create the first graph."
        : "Both planes are connected. Open a source file to create the first graph.",
      action: input.hasActiveFile
        ? { label: "Ingest active file", command: "mindleak.ingestActiveFile" }
        : { label: "Open a source file", command: "workbench.action.files.openFile" },
    };
  }

  if (input.actionableTasks > 0 || input.actionableDesigns > 0) {
    const openDesigns = input.actionableDesigns > 0;
    return {
      ...common,
      state: "coordinating",
      title: "Coordinating active work",
      detail: `${input.actionableTasks} actionable task(s); ${input.actionableDesigns} design decision(s).`,
      action: openDesigns
        ? { label: "Open Design Board", command: "mindleak.designView.focus" }
        : { label: "Open Intent Board", command: "mindleak.boardView.focus" },
    };
  }

  if (degradedCapabilities.length > 0) {
    return {
      ...common,
      state: "degraded_optional",
      title: "Core ready; optional capture limited",
      detail: degradedCapabilities.join("; "),
      action: { label: "Open Telemetry", command: "mindleak.telemetryView.focus" },
    };
  }

  return {
    ...common,
    state: "observing",
    title: "Observing workspace context",
    detail: `${graph.nodes} node(s) and ${graph.activeEdges} active edge(s) are available.`,
    action: { label: "Open Context Graph", command: "mindleak.graphView.focus" },
  };
}

export function readinessRows(snapshot: ReadinessSnapshot): ReadinessRow[] {
  const rows: ReadinessRow[] = [
    {
      id: "state",
      kind: "state",
      label: snapshot.title,
      description: snapshot.action.label,
      tooltip: snapshot.detail,
      icon: stateIcon(snapshot.state),
      action: snapshot.action,
    },
    {
      id: "memory",
      kind: "server",
      label: "Memory plane",
      description: snapshot.memory,
      tooltip: snapshot.memory,
      icon: snapshot.memory.includes("unavailable") ? "error" : "database",
    },
    {
      id: "intent",
      kind: "server",
      label: "Intent plane",
      description: snapshot.intent,
      tooltip: snapshot.intent,
      icon: snapshot.intent.includes("unavailable") ? "error" : "references",
    },
    {
      id: "agent",
      kind: "agent",
      label: "Agent identity",
      description: snapshot.agent,
      tooltip: `Attribution identity: ${snapshot.agent}`,
      icon: "account",
    },
  ];
  snapshot.degradedCapabilities.forEach((detail, index) => {
    rows.push({
      id: `optional-${index}`,
      kind: "optional",
      label: "Optional capability",
      description: detail,
      tooltip: detail,
      icon: "warning",
    });
  });
  return rows;
}

function serverLabel(
  plane: string,
  connected: boolean,
  identity: ServerIdentity | undefined
): string {
  if (!connected) {
    return `${plane} unavailable`;
  }
  return identity ? `${identity.name} ${identity.version}` : `${plane} connected`;
}

function optionalDegradations(input: ReadinessInput): string[] {
  return [input.terminalHealth, input.gitHealth].filter((health) =>
    /\b(?:degraded|disabled|unavailable)\b/i.test(health)
  );
}

function stateIcon(state: ReadinessState): string {
  switch (state) {
    case "disconnected":
      return "error";
    case "ready_empty":
      return "sparkle";
    case "coordinating":
      return "organization";
    case "degraded_optional":
      return "warning";
    case "observing":
      return "pulse";
  }
}
