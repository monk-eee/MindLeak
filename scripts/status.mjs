#!/usr/bin/env node
// A human-runnable read of live Lodestar/MindLeak state.
//
// Every number a person had about live state used to come from an agent
// relaying MCP tool output mid-conversation -- asking an agent to relay a
// deterministic local read is a slower, LLM-mediated way to answer a
// question that needs neither. `lodestar_stats`, `task_query`,
// `graph_stats`, and `telemetry_snapshot` all answer without an
// `open_session` or a claimed identity, which is what makes running the
// compiled server binaries directly -- the same way board-health.mjs and
// canonical-push already do -- sufficient here. No MCP client library beyond
// this repository's own, no LLM call, no agent conversation.

import { execFileSync } from "node:child_process";

import { isLive, isStrandedClaim } from "./board-health.mjs";
import { callTools, resolveServer } from "./claim-gate.mjs";

const repoRoot = execFileSync("git", ["rev-parse", "--show-toplevel"], {
  encoding: "utf8",
}).trim();

/**
 * The board's live (non-terminal) tasks, projected to exactly what a person
 * needs about a claim: who holds it, and whether its lease has already run
 * out (ADR-0048). `board` is `task_query view=board`'s full result -- every
 * task this repository has ever created, done or not -- so the narrowing
 * happens here rather than asking the server for a view that does not exist;
 * `isLive`/`isStrandedClaim` are the same predicates board-health.mjs already
 * trusts, reused rather than re-derived so the two reports cannot disagree
 * about what "lapsed" means.
 */
export function liveClaims(board, now) {
  return board.filter(isLive).map((task) => ({
    id: task.id,
    title: task.title,
    status: task.status,
    owner: task.owner,
    lease_expires_at: task.lease_expires_at,
    lapsed: isStrandedClaim(task, now),
  }));
}

/** One line per doctor finding, in the words `remedy` already supplies. */
function renderDoctor(findings) {
  if (!findings || findings.length === 0) return "  no board ailments";
  return findings
    .map(
      (finding) =>
        `  [${finding.ailment}] ${finding.subject} (${finding.task_ids.join(", ")}) -- ${finding.remedy}`,
    )
    .join("\n");
}

/** One line per live task, naming the lease state a claimed task is in. */
function renderClaims(claims) {
  if (claims.length === 0) {
    return "  nothing open, claimed, blocked, or in review";
  }
  return claims
    .map((claim) => {
      const state =
        claim.status !== "claimed"
          ? claim.status
          : claim.lapsed
            ? `claimed by ${claim.owner} -- LEASE LAPSED`
            : `claimed by ${claim.owner}`;
      return `  ${claim.id}  ${state}  ${claim.title}`;
    })
    .join("\n");
}

/**
 * The whole report as plain text, built from already-fetched, already-parsed
 * data so it is testable without spawning anything real. `graph`/`telemetry`
 * are optional: a checkout with no `mindleak-mcp` binary built yet still has
 * a Lodestar half worth reporting, and the report should say what it can
 * rather than fail outright over the half that is missing.
 */
export function formatReport({
  lodestarStats,
  doctor,
  claims,
  graph,
  telemetry,
  now,
}) {
  const lines = [
    `MindLeak/Lodestar status -- ${new Date(now * 1000).toISOString()}`,
    "",
    "Lodestar:",
    `  ${lodestarStats.active_goals} active goals, ${lodestarStats.open_tasks} open, ` +
      `${lodestarStats.claimed_tasks} claimed, ${lodestarStats.done_tasks} done, ` +
      `${lodestarStats.active_knowledge} active knowledge`,
    "Board doctor:",
    renderDoctor(doctor),
    "Live claims:",
    renderClaims(claims),
  ];
  if (graph) {
    lines.push(
      "MindLeak graph:",
      `  ${graph.nodes} nodes, ${graph.active_edges} active edges, ` +
        `${graph.unembedded_nodes} not recallable, ${graph.split_identity_nodes} split-identity`,
    );
  }
  if (telemetry) {
    const failing = telemetry.currently_failing_tools ?? 0;
    lines.push(
      "MindLeak telemetry:",
      `  ${telemetry.total_events ?? "?"} events, ` +
        `${failing} tool(s) currently failing`,
    );
  }
  return lines.join("\n");
}

const USAGE = `status -- a human-runnable read of live Lodestar/MindLeak state

  node scripts/status.mjs [--json]

Prints Lodestar board health (active goals, task counts, doctor findings,
live claims and their lease state) and MindLeak graph/telemetry health,
reading each plane's compiled server binary directly. No agent session, MCP
client, or LLM call is involved.

  --json   print the underlying data as JSON instead of the plain-text report
  --help   this message

Needs at least a lodestar-mcp build (cargo build -p lodestar-mcp, or set
LODESTAR_MCP_BIN); a mindleak-mcp build is optional -- its section is omitted
when the binary is not found.`;

async function main() {
  const args = process.argv.slice(2);
  if (args.includes("--help") || args.includes("-h")) {
    console.log(USAGE);
    return;
  }
  const json = args.includes("--json");

  const lodestarBin = resolveServer(repoRoot, "lodestar");
  if (!lodestarBin) {
    console.error(
      "status: no lodestar-mcp binary found.\n" +
        "  Build one:  cargo build -p lodestar-mcp\n" +
        "  Or point at one:  set LODESTAR_MCP_BIN",
    );
    process.exitCode = 2;
    return;
  }

  const [lodestarStats, doctor, board] = callTools(
    lodestarBin,
    repoRoot,
    [
      { name: "lodestar_stats" },
      { name: "task_query", arguments: { view: "doctor" } },
      { name: "task_query", arguments: { view: "board" } },
    ],
    // view=board returns every task this repository has ever created, done
    // or not; a mature board measures well past execFileSync's 1 MiB default.
    64 * 1024 * 1024,
  );

  const mindleakBin = resolveServer(repoRoot, "mindleak");
  let graph = null;
  let telemetry = null;
  if (mindleakBin) {
    [graph, telemetry] = callTools(mindleakBin, repoRoot, [
      { name: "graph_stats" },
      { name: "telemetry_snapshot" },
    ]);
  }

  const now = Math.floor(Date.now() / 1000);
  const claims = liveClaims(
    Array.isArray(board) ? board : Object.values(board),
    now,
  );

  if (json) {
    console.log(
      JSON.stringify(
        { lodestarStats, doctor, claims, graph, telemetry },
        null,
        2,
      ),
    );
    return;
  }
  console.log(
    formatReport({ lodestarStats, doctor, claims, graph, telemetry, now }),
  );
}

if (
  process.argv[1] &&
  import.meta.url.endsWith(process.argv[1].replace(/\\/g, "/"))
) {
  main().catch((error) => {
    console.error(`status: ${error.message}`);
    process.exitCode = 1;
  });
}
