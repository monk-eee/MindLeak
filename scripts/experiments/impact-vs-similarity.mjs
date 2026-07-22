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
// An OPTIONAL live-embedding arm runs real nomic-embed-text vectors when a local
// /v1/embeddings server is reachable, and is skipped otherwise so the core
// comparison stays deterministic and network-free.

import {
  resolveExe,
  driveServer,
  metrics,
  pct,
  cosineDense,
  embedAll,
  deriveImporters,
  transitiveImporters,
} from "./harness.mjs";

const root = process.cwd();
const exe = resolveExe(root);

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
  "src/format.ts":
    "export function formatName(first, last) { return `${first} ${last}`; }\n",
};

// ---- ground truth: transitive importers of the hub (depth <= 2) ------------
// Import-graph derivation is shared with the agent-outcome benchmark.
const importers = deriveImporters(fixture);
const groundTruth = transitiveImporters(importers, HUB, 2); // impact radius is depth 2

// ---- similarity arm: TF-IDF cosine over file contents ----------------------
function tokenize(text) {
  return (text.toLowerCase().match(/[a-z_][a-z0-9_]*/g) ?? []).filter(
    (t) => t.length > 1,
  );
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
    for (const [term, count] of tf)
      vec.set(term, (count / doc.length) * idf(term));
    return vec;
  };
  const vectors = docs.map(vector);
  const cosine = (a, b) => {
    let dot = 0;
    for (const [term, weight] of a) dot += weight * (b.get(term) ?? 0);
    const norm = (v) =>
      Math.sqrt([...v.values()].reduce((s, w) => s + w * w, 0));
    const denom = norm(a) * norm(b);
    return denom === 0 ? 0 : dot / denom;
  };
  const hubVec = vectors[files.indexOf(hub)];
  return files
    .map((file, i) => ({ file, score: cosine(hubVec, vectors[i]) }))
    .filter((entry) => entry.file !== hub)
    .sort((a, b) => b.score - a.score);
}

// ---- live-embedding arm: real vectors when a local server is reachable -----
async function embeddingRanking(files, hub) {
  const vectors = await embedAll(files.map((f) => fixture[f]));
  if (!vectors) return null; // no reachable /v1/embeddings server -> arm skipped
  const hubVec = vectors[files.indexOf(hub)];
  return files
    .map((file, i) => ({ file, score: cosineDense(hubVec, vectors[i]) }))
    .filter((entry) => entry.file !== hub)
    .sort((a, b) => b.score - a.score);
}

(async () => {
  const { request, tool, cleanup } = driveServer(exe, root);
  try {
    await request("initialize", {});
    for (const [file, content] of Object.entries(fixture)) {
      await tool("ingest_file", { path: file, content });
    }
    const impact = await tool("get_impact_radius", {
      target_artifact: `artifact:${HUB}`,
    });
    const graphPredicted = new Set(
      impact.nodes
        .map((node) => node.id)
        .filter((id) => id.startsWith("artifact:") && id !== `artifact:${HUB}`)
        .map((id) => id.slice("artifact:".length)),
    );

    const k = groundTruth.size;
    const ranking = tfidfRanking(Object.keys(fixture), HUB);
    const similarityPredicted = new Set(
      ranking.slice(0, k).map((entry) => entry.file),
    );

    const graph = metrics(graphPredicted, groundTruth);
    const similarity = metrics(similarityPredicted, groundTruth);

    const embModel = process.env.MINDLEAK_EMBED_MODEL ?? "nomic-embed-text";
    const embRanking = await embeddingRanking(Object.keys(fixture), HUB);
    const embedding = embRanking
      ? metrics(
          new Set(embRanking.slice(0, k).map((entry) => entry.file)),
          groundTruth,
        )
      : null;

    console.log(`\nQuery: "what breaks if I change ${HUB}?"`);
    console.log(
      `Ground truth (${groundTruth.size} transitive importers): ${[...groundTruth].sort().join(", ")}\n`,
    );

    console.log("| Method                     | Precision | Recall | F1   |");
    console.log("|----------------------------|-----------|--------|------|");
    console.log(
      `| MindLeak (graph impact)    |    ${pct(graph.precision).padStart(4)} |   ${pct(graph.recall).padStart(4)} | ${graph.f1.toFixed(2)} |`,
    );
    console.log(
      `| Similarity (TF-IDF top-${k})  |    ${pct(similarity.precision).padStart(4)} |   ${pct(similarity.recall).padStart(4)} | ${similarity.f1.toFixed(2)} |`,
    );
    if (embedding) {
      console.log(
        `| Embeddings (live top-${k})    |    ${pct(embedding.precision).padStart(4)} |   ${pct(embedding.recall).padStart(4)} | ${embedding.f1.toFixed(2)} |`,
      );
    }

    console.log(`\nMindLeak retrieved:   ${graph.predicted.join(", ")}`);
    console.log(`Similarity retrieved: ${similarity.predicted.join(", ")}`);
    if (embedding) {
      console.log(
        `Embeddings retrieved: ${embedding.predicted.join(", ")}  (backend: ${embModel})`,
      );
    } else {
      const embUrl =
        process.env.MINDLEAK_EMBED_URL ?? "http://localhost:11434/v1";
      console.log(
        `Embeddings arm skipped: no reachable ${embUrl}/embeddings server.`,
      );
    }
    const falsePositives = similarity.predicted.filter(
      (f) => !groundTruth.has(f),
    );
    console.log(
      `Similarity false positives (similar vocab, no import): ${falsePositives.join(", ") || "none"}`,
    );

    console.log(
      JSON.stringify(
        {
          experiment:
            "impact-precision: structural graph vs lexical similarity",
          query: `impact of changing ${HUB}`,
          ground_truth: [...groundTruth].sort(),
          graph,
          similarity: { ...similarity, backend: "tfidf-cosine" },
          embedding: embedding
            ? { ...embedding, backend: embModel }
            : { skipped: true },
        },
        null,
        2,
      ),
    );
  } finally {
    await cleanup();
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
