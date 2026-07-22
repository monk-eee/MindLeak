// Shared harness for MindLeak experiments (scripts/experiments/*).
//
// One place for the plumbing every experiment needs:
//   - locate + drive the mindleak-mcp server over stdio JSON-RPC;
//   - scoring (precision / recall / F1);
//   - lexical (TF-IDF) and optional dense (embedding) similarity;
//   - JS/TS import-graph ground truth.
//
// Cross-platform, zero build-time dependencies (Node stdlib + global fetch).

import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";

// ---- server plumbing -------------------------------------------------------

/** Absolute path to the built mindleak-mcp binary; exits(2) if missing. */
export function resolveExe(root = process.cwd()) {
  const exe = path.join(
    root,
    "target",
    "debug",
    process.platform === "win32" ? "mindleak-mcp.exe" : "mindleak-mcp",
  );
  if (!fs.existsSync(exe)) {
    console.error(
      `mindleak-mcp not built at ${exe}. Run: cargo build -p mindleak-mcp`,
    );
    process.exit(2);
  }
  return exe;
}

/**
 * Spawn mindleak-mcp against a fresh temp database and return newline-delimited
 * JSON-RPC helpers. `tool(name, args)` unwraps the tool content; `cleanup()`
 * ends the server and best-effort removes the temp dir.
 */
export function driveServer(exe, root = process.cwd(), extraEnv = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "mindleak-exp-"));
  const env = {
    ...process.env,
    MINDLEAK_DB: path.join(dir, "graph.db"),
    ...extraEnv,
  };
  delete env.MINDLEAK_AGENT; // no attribution noise in the graph
  const server = spawn(exe, [], {
    cwd: root,
    env,
    stdio: ["pipe", "pipe", "inherit"],
  });
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
      server.stdin.write(
        `${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`,
      );
    });
  const tool = async (name, args) => {
    const response = await request("tools/call", { name, arguments: args });
    if (response.error || response.result?.isError)
      throw new Error(JSON.stringify(response));
    return JSON.parse(response.result.content[0].text);
  };
  const cleanup = () =>
    new Promise((resolve) => {
      server.once("exit", () => {
        try {
          fs.rmSync(dir, {
            recursive: true,
            force: true,
            maxRetries: 5,
            retryDelay: 100,
          });
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

// ---- scoring ---------------------------------------------------------------

/** Precision / recall / F1 of a predicted node set against ground truth. */
export function metrics(predicted, truth) {
  const pred = predicted instanceof Set ? predicted : new Set(predicted);
  const gt = truth instanceof Set ? truth : new Set(truth);
  const hits = [...pred].filter((p) => gt.has(p));
  const precision = pred.size ? hits.length / pred.size : 0;
  const recall = gt.size ? hits.length / gt.size : 0;
  const f1 =
    precision + recall ? (2 * precision * recall) / (precision + recall) : 0;
  return { precision, recall, f1, predicted: [...pred].sort() };
}

export const pct = (x) => `${(x * 100).toFixed(0)}%`;

// ---- dense (embedding) similarity, optional --------------------------------

/** Cosine similarity of two equal-length dense vectors; 0 for degenerate input. */
export function cosineDense(a, b) {
  let dot = 0;
  let na = 0;
  let nb = 0;
  for (let i = 0; i < a.length; i++) {
    dot += a[i] * b[i];
    na += a[i] * a[i];
    nb += b[i] * b[i];
  }
  const denom = Math.sqrt(na) * Math.sqrt(nb);
  return denom === 0 ? 0 : dot / denom;
}

/**
 * Embed `text` via a local OpenAI-compatible /v1/embeddings server (Ollama by
 * default). Matches the Rust Embedder defaults so both sides agree. Throws on
 * any failure so callers can decide to skip the arm.
 */
export async function embed(text, opts = {}) {
  const base = (
    opts.url ??
    process.env.MINDLEAK_EMBED_URL ??
    "http://localhost:11434/v1"
  ).replace(/\/+$/, "");
  const model =
    opts.model ?? process.env.MINDLEAK_EMBED_MODEL ?? "nomic-embed-text";
  const apiKey = opts.apiKey ?? process.env.MINDLEAK_EMBED_API_KEY ?? "";
  const headers = { "Content-Type": "application/json" };
  if (apiKey) headers.Authorization = `Bearer ${apiKey}`;
  const resp = await fetch(`${base}/embeddings`, {
    method: "POST",
    headers,
    body: JSON.stringify({ model, input: text }),
  });
  if (!resp.ok) throw new Error(`embeddings HTTP ${resp.status}`);
  const json = await resp.json();
  const vec = json?.data?.[0]?.embedding;
  if (!Array.isArray(vec) || vec.length === 0)
    throw new Error("empty embedding vector");
  return vec;
}

/** Embed many texts; returns number[][] or null if the server is unreachable. */
export async function embedAll(texts, opts = {}) {
  try {
    const out = [];
    for (const text of texts) out.push(await embed(text, opts));
    return out;
  } catch {
    return null;
  }
}

// ---- lexical (TF-IDF) similarity ------------------------------------------

export function tokenize(text) {
  return (text.toLowerCase().match(/[a-z_][a-z0-9_]*/g) ?? []).filter(
    (t) => t.length > 1,
  );
}

/**
 * Build a TF-IDF ranker over a corpus (Map id -> text). The returned function
 * scores an arbitrary query string against every document by cosine similarity
 * and returns [{ id, score }] sorted descending. Smoothed idf so a term present
 * in every document still contributes a little.
 */
export function tfidfRanker(corpus) {
  const ids = [...corpus.keys()];
  const docs = ids.map((id) => tokenize(corpus.get(id)));
  const df = new Map();
  for (const doc of docs) {
    for (const term of new Set(doc)) df.set(term, (df.get(term) ?? 0) + 1);
  }
  const total = ids.length;
  const idf = (term) => Math.log((total + 1) / ((df.get(term) ?? 0) + 1)) + 1;
  const vectorize = (tokens) => {
    const tf = new Map();
    for (const term of tokens) tf.set(term, (tf.get(term) ?? 0) + 1);
    const vec = new Map();
    const len = tokens.length || 1;
    for (const [term, count] of tf) vec.set(term, (count / len) * idf(term));
    return vec;
  };
  const norm = (v) => Math.sqrt([...v.values()].reduce((s, w) => s + w * w, 0));
  const vectors = docs.map(vectorize);
  const norms = vectors.map(norm);
  return (queryText, { exclude } = {}) => {
    const qv = vectorize(tokenize(queryText));
    const qn = norm(qv);
    return ids
      .map((id, i) => {
        if (qn === 0 || norms[i] === 0) return { id, score: 0 };
        let dot = 0;
        for (const [term, weight] of qv)
          dot += weight * (vectors[i].get(term) ?? 0);
        return { id, score: dot / (qn * norms[i]) };
      })
      .filter((entry) => !exclude || !exclude.has(entry.id))
      .sort((a, b) => b.score - a.score);
  };
}

// ---- JS/TS import-graph ground truth --------------------------------------

/** Resolve a relative import specifier to a fixture file key, or null. */
export function resolveRelative(fixture, fromPath, specifier) {
  if (!specifier.startsWith(".")) return null; // bare package: not a workspace file
  const dir = fromPath.includes("/")
    ? fromPath.slice(0, fromPath.lastIndexOf("/"))
    : "";
  const parts = (dir ? `${dir}/${specifier}` : specifier).split("/");
  const stack = [];
  for (const part of parts) {
    if (part === "" || part === ".") continue;
    if (part === "..") stack.pop();
    else stack.push(part);
  }
  const base = stack.join("/");
  return Object.keys(fixture).find(
    (f) => f === base || f === `${base}.ts` || f === `${base}.tsx`,
  );
}

/** Map of target file -> Set of files that import it, from a { path: content } fixture. */
export function deriveImporters(fixture) {
  const importers = new Map();
  const importRe = /import\s+(?:[^'"]+from\s+)?['"]([^'"]+)['"]/g;
  for (const [file, content] of Object.entries(fixture)) {
    for (const match of content.matchAll(importRe)) {
      const target = resolveRelative(fixture, file, match[1]);
      if (!target) continue;
      if (!importers.has(target)) importers.set(target, new Set());
      importers.get(target).add(file);
    }
  }
  return importers;
}

/** Transitive importers of `hub` up to `maxDepth` hops. */
export function transitiveImporters(importers, hub, maxDepth) {
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
