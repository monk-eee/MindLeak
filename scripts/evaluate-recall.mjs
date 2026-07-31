// Measure recall against a populated real index, never a synthetic fixture.
// Usage:
//   node scripts/evaluate-recall.mjs --bin <mindleak-mcp> --db <graph.db>

/** Cosine similarity; 0 for empty or mismatched vectors. */
export function cosine(a, b) {
  if (!a.length || a.length !== b.length) return 0;
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

/** Mean, standard deviation, and the top score's distance above the mean. */
export function fieldStats(scores) {
  const n = scores.length;
  if (n === 0) return { mean: 0, sd: 0, top: 0, sigma: 0 };
  const mean = scores.reduce((a, b) => a + b, 0) / n;
  const sd = Math.sqrt(scores.reduce((a, s) => a + (s - mean) ** 2, 0) / n);
  const top = Math.max(...scores);
  return { mean, sd, top, sigma: sd === 0 ? 0 : (top - mean) / sd };
}

/**
 * Whether one threshold can separate two labelled bands.
 *
 * Returns the gap between the highest rejectable value and the lowest keepable
 * one. A non-positive gap means no single constant does the job — which is the
 * finding this harness was written to test, and the same shape as the recall
 * floor's own overlapping ranges.
 */
export function separation(reject, keep) {
  if (!reject.length || !keep.length) return { separable: false, gap: 0 };
  const highestReject = Math.max(...reject);
  const lowestKeep = Math.min(...keep);
  return {
    separable: highestReject < lowestKeep,
    gap: lowestKeep - highestReject,
    highestReject,
    lowestKeep,
  };
}

export function relevantHits(results, anchors) {
  return results.filter((result) => {
    const terms = new Set(
      `${result.label || ""} ${result.content || ""}`
        .toLowerCase()
        .split(/[^a-z0-9]+/)
        .filter(Boolean),
    );
    return anchors.some((anchor) => terms.has(anchor.toLowerCase()));
  });
}

/** Decode the f32 little-endian vector blob the index stores. */
export function fromBlob(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const out = new Float32Array(bytes.byteLength >> 2);
  for (let i = 0; i < out.length; i++) out[i] = view.getFloat32(i * 4, true);
  return out;
}

const NONSENSE = [
  "zzzzqqq wibble flarp",
  "qwertyuiop asdfghjkl zxcvbnm",
  "flurb glorp snizzle wexpo",
];

const ABSENT = [
  "how should PostgreSQL autovacuum be tuned for a billion row table",
  "where is the payroll tax withholding calculation implemented",
  "why does a Kubernetes ingress return a 502 response",
  "how do I rotate an AWS access key without downtime",
];

const REAL = [
  {
    query: "canonical-push auto-merge armed refuses",
    anchors: ["armed", "merge"],
  },
  {
    query: "what breaks when an agent commits before claiming the task",
    anchors: ["agent", "commits", "claiming"],
  },
  {
    query: "why does a lapsed lease stop the work certifying",
    anchors: ["lapsed", "lease", "certifying"],
  },
  {
    query: "why does PowerShell report failure for a command that succeeded",
    anchors: ["powershell", "succeeded"],
  },
  {
    query: "a test that passes locally but is red only on windows",
    anchors: ["windows", "red"],
  },
  {
    query: "why can a recorded lesson never reach another agent",
    anchors: ["lesson", "reach"],
  },
  {
    query: "the running server is behind the source it reports on",
    anchors: ["server", "behind", "source", "stale", "binary"],
  },
];

async function main() {
  // Imported here, never at module load: CI pins Node 20, where `node:sqlite`
  // does not exist, and a top-level import would kill the whole run on import.
  const { DatabaseSync } = await import("node:sqlite");
  const { spawn } = await import("node:child_process");
  const { execFileSync } = await import("node:child_process");
  const { createHash } = await import("node:crypto");
  const { readFileSync } = await import("node:fs");
  const { dirname, resolve } = await import("node:path");
  const { fileURLToPath } = await import("node:url");
  const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

  const argv = process.argv.slice(2);
  const arg = (name) => {
    const at = argv.indexOf(name);
    return at === -1 ? undefined : argv[at + 1];
  };
  const bin = arg("--bin");
  const db = arg("--db");
  const embedUrl =
    process.env.MINDLEAK_EMBED_URL || "http://localhost:11434/v1";
  const limit = Number(arg("--limit") || 5);
  const floor = Number(arg("--floor") || 0.5);
  if (!bin || !db) {
    console.error(
      "usage: evaluate-recall.mjs --bin <mindleak-mcp> --db <graph.db>",
    );
    process.exit(2);
  }

  const conn = new DatabaseSync(db, { readOnly: true });
  const models = conn
    .prepare(
      "select model, count(*) c from embeddings group by model order by c desc",
    )
    .all();
  if (!models.length) {
    console.error("evaluate-recall: the index is empty; run `index` first");
    process.exit(2);
  }
  const model = models[0].model;
  const kind = new Map();
  for (const row of conn.prepare("select id, type from nodes").all()) {
    kind.set(row.id, row.type);
  }
  const rows = conn
    .prepare("select node_id, vector from embeddings where model = ?")
    .all(model)
    .map((r) => ({ id: r.node_id, v: fromBlob(r.vector) }));

  const embed = async (text) => {
    const res = await fetch(embedUrl + "/embeddings", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ model, input: text }),
    });
    if (!res.ok) throw new Error("embeddings server returned " + res.status);
    return Float32Array.from((await res.json()).data[0].embedding);
  };

  const askShipped = (query) =>
    new Promise((resolve) => {
      const child = spawn(bin, [], { stdio: ["pipe", "pipe", "ignore"] });
      let out = "";
      child.stdout.on("data", (c) => (out += c));
      child.on("error", () => resolve({ results: [] }));
      child.on("close", () => {
        for (const line of out.split(/\r?\n/)) {
          if (!line.trim()) continue;
          let message;
          try {
            message = JSON.parse(line);
          } catch {
            continue;
          }
          if (message.id === 2) {
            try {
              return resolve(JSON.parse(message.result.content[0].text));
            } catch {
              return resolve({ results: [] });
            }
          }
        }
        resolve({ results: [] });
      });
      const requests = [
        { jsonrpc: "2.0", id: 0, method: "initialize", params: {} },
        {
          jsonrpc: "2.0",
          id: 2,
          method: "tools/call",
          params: { name: "recall", arguments: { query, limit } },
        },
      ];
      child.stdin.write(
        requests.map((r) => JSON.stringify(r)).join("\n") + "\n",
      );
      child.stdin.end();
    });

  const served = { before: {}, after: {} };
  const dangling = { before: 0, after: 0 };
  const total = { before: 0, after: 0 };
  const sigmas = { nonsense: [], absent: [], real: [] };
  const abstention = { controls: 0, abstained: 0 };
  const realAnswers = { queries: 0, nonempty: 0, relevant: 0 };
  const perQuery = [];

  for (const [label, queries] of [
    ["nonsense", NONSENSE],
    ["absent", ABSENT],
    ["real", REAL],
  ]) {
    for (const item of queries) {
      const query = typeof item === "string" ? item : item.query;
      const anchors = typeof item === "string" ? [] : item.anchors;
      const qv = await embed(query);
      const scores = rows.map((r) => cosine(qv, r.v));
      const stats = fieldStats(scores);
      sigmas[label].push(stats.sigma);

      const before = rows
        .map((r, i) => ({ id: r.id, score: scores[i] }))
        .filter((r) => r.score >= floor)
        .sort((a, b) => b.score - a.score)
        .slice(0, limit);
      const after = (await askShipped(query)).results || [];
      const relevantAfter = relevantHits(after, anchors);

      if (label === "real") {
        realAnswers.queries++;
        if (after.length) realAnswers.nonempty++;
        if (relevantAfter.length) realAnswers.relevant++;
      } else {
        abstention.controls++;
        if (!after.length) abstention.abstained++;
      }

      for (const hit of before) {
        const k = kind.get(hit.id);
        served.before[k || "gone"] = (served.before[k || "gone"] || 0) + 1;
        if (!k) dangling.before++;
        total.before++;
      }
      for (const hit of after) {
        const k = kind.get(hit.id) || hit.type;
        served.after[k || "gone"] = (served.after[k || "gone"] || 0) + 1;
        if (!kind.get(hit.id)) dangling.after++;
        total.after++;
      }

      perQuery.push({
        query,
        label,
        field_mean: stats.mean,
        field_sd: stats.sd,
        top_score: stats.top,
        top_sigma_above_field: stats.sigma,
        before_hits: before.length,
        after_hits: after.length,
        after_relevant_hits: relevantAfter.length,
      });
      console.log(
        label.padEnd(9) +
          ("sigma " + stats.sigma.toFixed(2)).padEnd(12) +
          ("before " + before.length).padEnd(10) +
          ("after " + after.length).padEnd(9) +
          query.slice(0, 46),
      );
    }
  }

  const sep = separation(sigmas.nonsense, sigmas.real);
  let revision = "unknown";
  let sourceWorktreeDirty = null;
  try {
    revision = execFileSync(
      "git",
      ["-C", repositoryRoot, "rev-parse", "--short", "HEAD"],
      {
        encoding: "utf8",
      },
    ).trim();
    sourceWorktreeDirty = Boolean(
      execFileSync("git", ["-C", repositoryRoot, "status", "--porcelain"], {
        encoding: "utf8",
      }).trim(),
    );
  } catch {
    // Not a checkout; the artifact still records everything else.
  }

  const artifact = {
    schema_version: 2,
    captured_at: new Date().toISOString().slice(0, 10),
    source_revision: revision,
    source_worktree_dirty: sourceWorktreeDirty,
    binary_sha256: createHash("sha256").update(readFileSync(bin)).digest("hex"),
    model,
    indexed_nodes: rows.length,
    floor,
    limit,
    served_by_kind: served,
    hits_naming_a_node_the_graph_no_longer_holds: {
      before: dangling.before,
      before_of: total.before,
      after: dangling.after,
      after_of: total.after,
    },
    control_abstention: abstention,
    real_answers: realAnswers,
    nonsense_rejection: {
      separable_by_one_sigma_threshold: sep.separable,
      highest_nonsense_sigma: sep.highestReject,
      lowest_real_sigma: sep.lowestKeep,
      margin: sep.gap,
    },
    per_query: perQuery,
  };
  const out = arg("--out");
  if (out) {
    const { writeFileSync } = await import("node:fs");
    writeFileSync(out, JSON.stringify(artifact, null, 2) + "\n");
    console.log("\nwrote " + out);
  } else {
    console.log("\n" + JSON.stringify(artifact, null, 2));
  }
}

if (import.meta.filename === process.argv[1]) {
  await main();
}
