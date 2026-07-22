// Impact-precision head-to-head: structural graph (MindLeak) vs lexical
// similarity ("vector-only" proxy) at answering "what breaks if I change X?".
//
// Deterministic, cross-platform, no network/model dependency. Builds a JS/TS
// fixture whose import graph gives the ground-truth impact set, then compares:
//   - MindLeak: get_impact_radius over the real `imports`/`calls` edges;
//   - Similarity: TF-IDF cosine top-k over file contents.
//
// The fixture is engineered to be honest and adversarial to BOTH methods:
//   - distractors share the changed file's vocabulary but do NOT import it
//     (similarity ranks them high -> false positives);
//   - true importers use different vocabulary (similarity ranks them low ->
//     misses), but the graph links them by the actual import edge.
//
// TF-IDF is a conservative (generous) stand-in for embeddings here: real
// embeddings would rank the vocabulary-sharing distractors even higher, so the
// structural advantage shown below is a lower bound, not an artefact of TF-IDF.

import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";

const root = process.cwd();
const exe = path.join(
  root,
  "target",
  "debug",
  process.platform === "win32" ? "mindleak-mcp.exe" : "mindleak-mcp"
);

if (!fs.existsSync(exe)) {
  console.error(`mindleak-mcp not built at ${exe}. Run: cargo build -p mindleak-mcp`);
  process.exit(2);
}

// ---- fixture: path -> content ---------------------------------------------
// hub is the "changed" file. Ground truth is derived from imports, below.
const HUB = "src/auth.ts";
const fixture = {
  "src/auth.ts":
    "// session token validation\n" +
    "export function validateSession(token) {\n" +
    "  // verify jwt token and check session expiry\n" +
    "  return Boolean(token);\n}\n",
  // --- true importers (different vocabulary from the hub) ---
  "src/login.ts":
    "import { validateSession } from './auth';\n" +
    "export function handleLogin(request) {\n" +
    "  // process credentials then redirect\n" +
    "  return validateSession(request.ticket);\n}\n",
  "src/middleware.ts":
    "import { validateSession } from './auth';\n" +
    "export function requireAuth(request, response, next) {\n" +
    "  if (!validateSession(request.ticket)) { response.status = 401; return; }\n" +
    "  next();\n}\n",
  "src/api/session-route.ts":
    "import { validateSession } from '../auth';\n" +
    "export function sessionRoute(endpoint) {\n" +
    "  // GET handler returning the current holder\n" +
    "  return validateSession(endpoint.ticket);\n}\n",
  "src/app.ts":
    "import { requireAuth } from './middleware';\n" +
    "export function bootstrap(server) {\n" +
    "  // start server and listen on a port\n" +
    "  server.use(requireAuth);\n}\n",
  // --- distractors (share the hub's vocabulary, but import nothing) ---
  "src/auth-legacy.ts":
    "// legacy session token validation\n" +
    "export function validateSessionLegacy(token) {\n" +
    "  // verify jwt token and check session expiry, old path\n" +
    "  return Boolean(token);\n}\n",
  "src/token-utils.ts":
    "// token and session helpers\n" +
    "export function parseToken(token) {\n" +
    "  // decode jwt token and read session expiry claims\n" +
    "  return { token, session: true };\n}\n",
  "src/auth-notes.ts":
    "// notes on session token validation and jwt expiry\n" +
    "export const sessionTokenNotes = 'validate token, check session, jwt expiry';\n",
  // --- unrelated ---
  "src/math.ts": "export function add(a, b) { return a + b; }\n",
  "src/format.ts": "export function formatName(first, last) { return `${first} ${last}`; }\n",
};

// ---- ground truth: transitive importers of the hub (depth <= 2) ------------
function resolveRelative(fromPath, specifier) {
  if (!specifier.startsWith(".")) return null; // bare package: not a workspace file
  const dir = fromPath.includes("/") ? fromPath.slice(0, fromPath.lastIndexOf("/")) : "";
  const parts = (dir ? `${dir}/${specifier}` : specifier).split("/");
  const stack = [];
  for (const part of parts) {
    if (part === "" || part === ".") continue;
    if (part === "..") stack.pop();
    else stack.push(part);
  }
  const base = stack.join("/");
  return Object.keys(fixture).find((f) => f === base || f === `${base}.ts` || f === `${base}.tsx`);
}

// importer edges: target file -> [files that import it]
const importers = new Map();
const importRe = /import\s+(?:[^'"]+from\s+)?['"]([^'"]+)['"]/g;
for (const [file, content] of Object.entries(fixture)) {
  for (const match of content.matchAll(importRe)) {
    const target = resolveRelative(file, match[1]);
    if (!target) continue;
    if (!importers.has(target)) importers.set(target, new Set());
    importers.get(target).add(file);
  }
}

function transitiveImporters(hub, maxDepth) {
  const found = new Set();
  let frontier = [hub];
  for (let depth = 0; depth < maxDepth; depth++) {
    const next = [];
    for (const node of frontier) {
      for (const importer of importers.get(node) ?? []) {
        if (!found.has(importer)) {
          found.add(importer);
          next.push(importer);
        }
      }
    }
    frontier = next;
  }
  return found;
}

const groundTruth = transitiveImporters(HUB, 2); // get_impact_radius is depth 2

// ---- similarity arm: TF-IDF cosine over file contents ----------------------
function tokenize(text) {
  return (text.toLowerCase().match(/[a-z_][a-z0-9_]*/g) ?? []).filter((t) => t.length > 1);
}

function tfidfRanking(files, hub) {
  const docs = files.map((f) => tokenize(fixture[f]));
  const df = new Map();
  for (const doc of docs) {
    for (const term of new Set(doc)) df.set(term, (df.get(term) ?? 0) + 1);
  }
  const idf = (term) => Math.log(files.length / (df.get(term) ?? files.length));
  const vector = (doc) => {
    const tf = new Map();
    for (const term of doc) tf.set(term, (tf.get(term) ?? 0) + 1);
    const vec = new Map();
    for (const [term, count] of tf) vec.set(term, (count / doc.length) * idf(term));
    return vec;
  };
  const vectors = docs.map(vector);
  const cosine = (a, b) => {
    let dot = 0;
    for (const [term, weight] of a) dot += weight * (b.get(term) ?? 0);
    const norm = (v) => Math.sqrt([...v.values()].reduce((s, w) => s + w * w, 0));
    const denom = norm(a) * norm(b);
    return denom === 0 ? 0 : dot / denom;
  };
  const hubVec = vectors[files.indexOf(hub)];
  return files
    .map((file, i) => ({ file, score: cosine(hubVec, vectors[i]) }))
    .filter((entry) => entry.file !== hub)
    .sort((a, b) => b.score - a.score);
}

// ---- MindLeak arm: drive the MCP server ------------------------------------
function driveServer() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "mindleak-impact-"));
  const env = { ...process.env, MINDLEAK_DB: path.join(dir, "graph.db") };
  delete env.MINDLEAK_AGENT; // no attribution noise
  const server = spawn(exe, [], { cwd: root, env, stdio: ["pipe", "pipe", "inherit"] });
  let nextId = 1;
  const pending = new Map();
  readline.createInterface({ input: server.stdout }).on("line", (line) => {
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      return;
    }
    const resolve = pending.get(message.id);
    if (resolve) {
      pending.delete(message.id);
      resolve(message);
    }
  });
  const request = (method, params) =>
    new Promise((resolve) => {
      const id = nextId++;
      pending.set(id, resolve);
      server.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
    });
  const tool = async (name, args) => {
    const response = await request("tools/call", { name, arguments: args });
    if (response.error || response.result?.isError) throw new Error(JSON.stringify(response));
    return JSON.parse(response.result.content[0].text);
  };
  const cleanup = () =>
    new Promise((resolve) => {
      server.once("exit", () => {
        try {
          fs.rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
        } catch {
          /* best-effort: the OS reclaims the temp dir */
        }
        resolve();
      });
      server.stdin.end();
      server.kill();
    });
  return { request, tool, cleanup };
}

function metrics(predicted, truth) {
  const hits = [...predicted].filter((p) => truth.has(p));
  const precision = predicted.size ? hits.length / predicted.size : 0;
  const recall = truth.size ? hits.length / truth.size : 0;
  const f1 = precision + recall ? (2 * precision * recall) / (precision + recall) : 0;
  return { precision, recall, f1, predicted: [...predicted].sort() };
}

const pct = (x) => `${(x * 100).toFixed(0)}%`;

(async () => {
  const { request, tool, cleanup } = driveServer();
  try {
    await request("initialize", {});
    for (const [file, content] of Object.entries(fixture)) {
      await tool("ingest_file", { path: file, content });
    }
    const impact = await tool("get_impact_radius", { target_artifact: `artifact:${HUB}` });
    const graphPredicted = new Set(
      impact.nodes
        .map((node) => node.id)
        .filter((id) => id.startsWith("artifact:") && id !== `artifact:${HUB}`)
        .map((id) => id.slice("artifact:".length))
    );

    const k = groundTruth.size;
    const ranking = tfidfRanking(Object.keys(fixture), HUB);
    const similarityPredicted = new Set(ranking.slice(0, k).map((entry) => entry.file));

    const graph = metrics(graphPredicted, groundTruth);
    const similarity = metrics(similarityPredicted, groundTruth);

    console.log(`\nQuery: "what breaks if I change ${HUB}?"`);
    console.log(`Ground truth (${groundTruth.size} transitive importers): ${[...groundTruth].sort().join(", ")}\n`);

    console.log("| Method                    | Precision | Recall | F1   |");
    console.log("|---------------------------|-----------|--------|------|");
    console.log(
      `| MindLeak (graph impact)   |    ${pct(graph.precision).padStart(4)} |   ${pct(graph.recall).padStart(4)} | ${graph.f1.toFixed(2)} |`
    );
    console.log(
      `| Similarity (TF-IDF top-${k}) |    ${pct(similarity.precision).padStart(4)} |   ${pct(similarity.recall).padStart(4)} | ${similarity.f1.toFixed(2)} |`
    );

    console.log(`\nMindLeak retrieved:   ${graph.predicted.join(", ")}`);
    console.log(`Similarity retrieved: ${similarity.predicted.join(", ")}`);
    const falsePositives = similarity.predicted.filter((f) => !groundTruth.has(f));
    console.log(`Similarity false positives (similar vocab, no import): ${falsePositives.join(", ") || "none"}`);

    console.log(
      JSON.stringify(
        {
          experiment: "impact-precision: structural graph vs lexical similarity",
          query: `impact of changing ${HUB}`,
          ground_truth: [...groundTruth].sort(),
          graph,
          similarity: { ...similarity, backend: "tfidf-cosine" },
        },
        null,
        2
      )
    );
  } finally {
    await cleanup();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
