// Event classification for the agent-in-the-loop benchmark.
//
// Extracted from `evaluate-agent-loop.mjs` so it can be tested without running
// the benchmark: that script spawns the Copilot CLI at import time, so a test
// importing it would start a real evaluation.

/// Tool calls that count as the agent exploring rather than editing.
///
/// The plane prefixes (`mindleak-eval-`, `lodestar-eval-`) are how the fixture
/// names its seeded MCP servers, so the classifier sees e.g.
/// `lodestar-eval-task_query`, not `task_query`.
const GENERIC_EXPLORATION = [
  "view",
  "grep",
  "glob",
  "shell",
  "bash",
  "powershell",
];

const MEMORY_EXPLORATION = /(graph_|impact_radius|recall|evidence_for)/;

/// Both vocabularies, deliberately.
///
/// ADR-0059 collapsed the Intent Plane surface, so the server the agent talks
/// to advertises `task_query`, `task_create`, `task_claim`, `task_transition`
/// and the `design_*` verbs. Matching only the retired names meant every
/// coordination call the agent made after the collapse went uncounted — and a
/// name-keyed classifier cannot report that it stopped matching: it returns
/// false, the run completes, and the exploration figure is simply lower.
///
/// The retired names stay so that runs either side of the deprecation window
/// remain comparable with `benchmarks/results/2026-07-22-agent-loop-outcome.json`.
/// Keeping this a superset is what makes fixing the counter a fix rather than
/// a silent re-definition of what the benchmark measures.
const INTENT_EXPLORATION =
  /(constitution|active_knowledge|task_query|task_create|task_claim|task_transition|design_query|design_register|design_decide|design_promote|board|next_task)/;

export function isExploration(name) {
  if (GENERIC_EXPLORATION.includes(name)) {
    return true;
  }
  if (name.startsWith("mindleak-eval-")) {
    return MEMORY_EXPLORATION.test(name);
  }
  if (name.startsWith("lodestar-eval-")) {
    return INTENT_EXPLORATION.test(name);
  }
  return false;
}

export function summarizeEvents(events) {
  const starts = events.filter(
    (event) => event.type === "tool.execution_start",
  );
  const toolNames = starts.map((event) => event.data?.toolName ?? "unknown");
  const exploration = toolNames.filter(isExploration);
  const outputTokens = events
    .filter((event) => event.type === "assistant.message")
    .reduce((sum, event) => sum + (event.data?.outputTokens ?? 0), 0);
  const final = [...events].reverse().find((event) => event.type === "result");
  return {
    model: events.find((event) => event.type === "session.tools_updated")?.data
      ?.model,
    total_tool_calls: toolNames.length,
    exploration_tool_calls: exploration.length,
    tool_names: toolNames,
    output_tokens: outputTokens,
    premium_requests: final?.usage?.premiumRequests ?? null,
    api_duration_ms: final?.usage?.totalApiDurationMs ?? null,
  };
}
