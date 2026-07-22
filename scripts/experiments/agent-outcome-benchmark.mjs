// Agent-outcome benchmark: does the memory backend surface the RIGHT context
// for an agent's task? One realistic workspace is ingested (structural imports,
// a failing execution, and an architectural decision), then four memory arms
// are asked to supply context for three tasks:
//
//   none     - no memory at all (the floor);
//   flat     - flat recency log: the k most-recent nodes, query-blind
//              (what append-only / flat-log agent memory returns);
//   vector   - semantic similarity: the `recall` tool over the local embedding
//              index when reachable, else TF-IDF over node text ("vector-only");
//   mindleak - the decay-weighted graph via the query-appropriate tool
//              (get_impact_radius for blast-radius, multi_hop for the rest).
//
// Each arm returns at most K=5 node ids (a realistic "context window" of items an
// agent reads). We score precision@K / recall / F1 against deterministic ground
// truth and report the mean F1 per arm. The point is not that vectors are
// useless - it is that flat recency and pure similarity are query-shaped wrong
// for structural/temporal questions, while the graph answers each in kind.
//
// Cross-platform, no build-time deps. The vector arm auto-upgrades to real
// embeddings when a local /v1/embeddings server (e.g. Ollama nomic-embed-text)
// is reachable, and falls back to TF-IDF otherwise so the run stays reproducible.

import fs from "node:fs";
import path from "node:path";
import {
  resolveExe,
  driveServer,
  metrics,
  tfidfRanker,
  deriveImporters,
  transitiveImporters,
} from "./harness.mjs";

const root = process.cwd();
const exe = resolveExe(root);
const K = 5; // context budget: how many items the agent gets to read

// ---- workspace fixture: structural + episodic + intent signals -------------
const HUB = "src/auth.ts";
const fixture = {
  "src/auth.ts":
    "// session token validation\n" +
    "export function validateSession(token) {\n" +
    "  return Boolean(token);\n}\n",
  // true importers (different vocabulary from the hub)
  "src/login.ts":
    "import { validateSession } from './auth';\n" +
    "export function handleLogin(request) {\n" +
    "  return validateSession(request.ticket);\n}\n",
  "src/middleware.ts":
    "import { validateSession } from './auth';\n" +
    "export function requireAuth(request, response, next) {\n" +
    "  if (!validateSession(request.ticket)) { response.status = 401; return; }\n" +
    "  next();\n}\n",
  "src/app.ts":
    "import { requireAuth } from './middleware';\n" +
    "export function bootstrap(server) {\n" +
    "  server.use(requireAuth);\n}\n",
  // distractors: share the hub's vocabulary but import nothing
  "src/auth-legacy.ts":
    "// legacy session token validation\n" +
    "export function validateSessionLegacy(token) {\n" +
    "  return Boolean(token);\n}\n",
  "src/token-utils.ts":
    "// token and session helpers\n" +
    "export function parseToken(token) {\n" +
    "  return { token, session: true };\n}\n",
  // unrelated
  "src/math.ts": "export function add(a, b) { return a + b; }\n",
  "src/format.ts": "export function formatName(first, last) { return `${first} ${last}`; }\n",
};

const importers = deriveImporters(fixture);
const impactTruth = new Set([...transitiveImporters(importers, HUB, 2)].map((f) => `artifact:${f}`));

const pctF1 = (x) => x.toFixed(2);

(async () => {
  const { request, tool, cleanup } = driveServer(exe, root);
  try {
    await request("initialize", {});

    // --- ingest the workspace (structural) ---
    for (const [file, content] of Object.entries(fixture)) {
      await tool("ingest_file", { path: file, content });
    }

    // --- episodic signal: a failing login test ---
    const execOutcome = await tool("ingest_execution", {
      command: "npm test -- login",
      exit_code: 1,
      output:
        "FAIL src/login.test.ts\n" +
        "  handleLogin > rejects an undefined session ticket\n" +
        "    at handleLogin (src/login.ts:2:10)\n" +
        "    at Object.<anonymous> (src/login.test.ts:5:3)\n" +
        "Error: session ticket was undefined\n",
      changed_files: ["src/login.ts"],
    });
    const executionId = (execOutcome.node_ids ?? []).find((id) => id.startsWith("execution:"));

    // --- intent signal: an architectural decision ---
    const decision = await tool("record_architectural_decision", {
      decision_text:
        "DECISION: session tokens must stay typed end to end; auth and middleware " +
        "must never accept stringly-typed tickets.",
      related_nodes: ["artifact:src/auth.ts", "artifact:src/middleware.ts"],
    });
    const intentId = decision.intent_id;

    // --- tasks with deterministic ground truth ---
    const tasks = [
      {
        name: "impact",
        question: "what breaks if I change session validation in src/auth.ts",
        truth: impactTruth,
        async mindleak() {
          const sub = await tool("get_impact_radius", { target_artifact: `artifact:${HUB}` });
          return sub.nodes
            .map((node) => node.id)
            .filter((id) => id.startsWith("artifact:") && id !== `artifact:${HUB}`);
        },
      },
      {
        name: "debug",
        question: "why is login failing with an undefined session ticket",
        truth: new Set([executionId, "artifact:src/login.ts"].filter(Boolean)),
        async mindleak() {
          const sub = await tool("graph_multi_hop_query", {
            seed_entity: "login failing session ticket undefined",
            max_depth: 2,
            min_weight: 0.05,
          });
          return sub.nodes.map((node) => node.id);
        },
      },
      {
        name: "rationale",
        question: "why must session tokens stay typed in auth and middleware",
        truth: new Set([intentId, "artifact:src/auth.ts", "artifact:src/middleware.ts"].filter(Boolean)),
        async mindleak() {
          const sub = await tool("graph_multi_hop_query", {
            seed_entity: "session tokens stay typed",
            max_depth: 2,
            min_weight: 0.05,
          });
          return sub.nodes.map((node) => node.id);
        },
      },
    ];

    // --- query-blind + lexical universe (one snapshot of every node) ---
    const snapshot = await tool("graph_snapshot", { limit: 500 });
    const allNodes = snapshot.nodes ?? [];
    const corpus = new Map(allNodes.map((node) => [node.id, `${node.label} ${node.content ?? ""}`]));
    const rankLexical = tfidfRanker(corpus);
    const recentIds = [...allNodes]
      .sort((a, b) => b.created_at - a.created_at || b.last_accessed_at - a.last_accessed_at)
      .map((node) => node.id);

    // --- vector backend: prefer the real embedding index, fall back to TF-IDF ---
    let vectorBackend = "tfidf-cosine";
    try {
      await tool("index", { limit: 500 });
      await tool("recall", { query: "warmup", limit: 1 });
      vectorBackend = process.env.MINDLEAK_EMBED_MODEL ?? "nomic-embed-text";
    } catch {
      vectorBackend = "tfidf-cosine";
    }

    const arms = {
      none: async () => [],
      flat: async () => recentIds.slice(0, K),
      vector: async (task) => {
        if (vectorBackend !== "tfidf-cosine") {
          const res = await tool("recall", { query: task.question, limit: K });
          return (res.results ?? []).map((entry) => entry.node.id);
        }
        return rankLexical(task.question).slice(0, K).map((entry) => entry.id);
      },
      mindleak: async (task) => (await task.mindleak()).slice(0, K),
    };
    const armNames = ["none", "flat", "vector", "mindleak"];

    // --- run every arm on every task ---
    const perArm = Object.fromEntries(armNames.map((arm) => [arm, { tasks: {}, meanF1: 0 }]));
    for (const task of tasks) {
      for (const arm of armNames) {
        const predicted = new Set(await arms[arm](task));
        perArm[arm].tasks[task.name] = metrics(predicted, task.truth);
      }
    }
    for (const arm of armNames) {
      const f1s = tasks.map((task) => perArm[arm].tasks[task.name].f1);
      perArm[arm].meanF1 = f1s.reduce((sum, x) => sum + x, 0) / f1s.length;
    }

    // --- report ---
    console.log(`\nAgent-outcome benchmark  (context budget K=${K}, vector backend: ${vectorBackend})`);
    for (const task of tasks) {
      console.log(
        `  task "${task.name}": ${task.question}\n    ground truth: ${[...task.truth].sort().join(", ")}`
      );
    }
    console.log("\n| Memory arm | impact F1 | debug F1 | rationale F1 | mean F1 |");
    console.log("|------------|-----------|----------|--------------|---------|");
    for (const arm of armNames) {
      const t = perArm[arm].tasks;
      console.log(
        `| ${arm.padEnd(10)} |      ${pctF1(t.impact.f1)} |     ${pctF1(t.debug.f1)} |         ${pctF1(t.rationale.f1)} |    ${pctF1(perArm[arm].meanF1)} |`
      );
    }

    console.log("\nWhat each arm retrieved per task:");
    for (const task of tasks) {
      console.log(`  ${task.name}:`);
      for (const arm of armNames) {
        console.log(`    ${arm.padEnd(9)} ${perArm[arm].tasks[task.name].predicted.join(", ") || "(nothing)"}`);
      }
    }

    const report = {
      experiment: "agent-outcome: memory-arm context precision",
      k: K,
      vector_backend: vectorBackend,
      tasks: tasks.map((task) => ({ name: task.name, question: task.question, truth: [...task.truth].sort() })),
      arms: perArm,
    };
    console.log(`\n${JSON.stringify(report, null, 2)}`);

    const outDir = path.join(root, "benchmarks", "results");
    fs.mkdirSync(outDir, { recursive: true });
    const outFile = path.join(outDir, "2026-07-22-agent-outcome-context-precision.json");
    fs.writeFileSync(outFile, `${JSON.stringify(report, null, 2)}\n`);
    console.log(`\nWrote ${path.relative(root, outFile)}`);
  } finally {
    await cleanup();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
