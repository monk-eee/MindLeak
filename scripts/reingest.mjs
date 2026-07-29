// Re-ingest the workspace so an extractor improvement reaches the graph that
// already exists.
//
// Structural extraction happens once, at ingest time. `ingest_file` writes the
// symbols, imports and calls the extractor could see *on the day it ran*, and
// nothing revisits them: `reconcile_workspace` only forgets files that vanished,
// and `index` only fills embeddings. So when the extractor learns something new
// — Rust `mod`/`use` edges, a new language, a fixed pattern — the existing graph
// does not learn it. Each file catches up only if somebody happens to save it.
//
// Measured 2026-07-29 on this repository, after Rust import extraction shipped:
// `get_impact_radius` on `crates/mindleak-core/src/model.rs`, which nearly every
// module in the crate imports, returned 11 nodes, 11 edges, and **zero** imports
// edges — the improvement was real and entirely invisible, because those 3,703
// artifact nodes were written by the old extractor.
//
// Re-ingesting is safe by construction: `replace_structure` atomically replaces
// everything an artifact emitted, and there are regression tests that a second
// ingest retracts structure the file no longer has.
//
// It is not free, though, and the cost is stated rather than hidden: re-asserting
// a structural edge resets its decay clock. That is honest for structure, which
// is true exactly as long as the file says so, but it does mean the structural
// tier will look uniformly freshly-observed afterwards. Attention edges
// (`observed`) are not written by this pass, so what the graph knows about who
// was working where is untouched.
//
// Cross-platform, dependency-free Node (toolchain rule).
//
// Usage:
//   node scripts/reingest.mjs                 # re-ingest into this repo's graph
//   node scripts/reingest.mjs --dry-run       # list what would be sent, touch nothing
//   node scripts/reingest.mjs --limit 50      # stop after N files

import { execFileSync, spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import readline from "node:readline";

/**
 * Extensions the deterministic extractor understands, plus the manifests it
 * reads for dependency edges. Sending it anything else costs a round trip and
 * writes an artifact node with no structure, which is worse than skipping:
 * a structureless node is indistinguishable from a file that genuinely has
 * none.
 */
export const EXTRACTABLE = new Set([
  "rs",
  "ts",
  "tsx",
  "js",
  "jsx",
  "mjs",
  "cjs",
  "py",
  "cs",
  "go",
  "java",
  "kt",
]);

export const MANIFESTS = new Set([
  "Cargo.toml",
  "package.json",
  "go.mod",
  "requirements.txt",
]);

/**
 * Directory segments that never belong in a code-context graph. Mirrors
 * `ingest::IGNORED_SEGMENTS` in `crates/mindleak-core/src/ingest/mod.rs`; the
 * server rejects these anyway, so matching here saves the round trip.
 */
export const IGNORED_SEGMENTS = new Set([
  ".git",
  "target",
  "node_modules",
  "dist",
  "coverage",
  ".mindleak",
  ".lodestar",
  ".vscode-test",
]);

/** True when a path lives under a junk directory, in any position. */
export function isIgnoredPath(filePath) {
  return filePath
    .replace(/\\/g, "/")
    .split("/")
    .some((segment) => IGNORED_SEGMENTS.has(segment));
}

/** True when the extractor has something to say about this file. */
export function isExtractable(filePath) {
  const clean = filePath.replace(/\\/g, "/");
  const name = clean.split("/").pop() ?? clean;
  if (MANIFESTS.has(name)) {
    return true;
  }
  // A dotfile with no extension (`.gitignore`) must not read as extension "gitignore".
  const dot = name.lastIndexOf(".");
  if (dot <= 0) {
    return false;
  }
  return EXTRACTABLE.has(name.slice(dot + 1).toLowerCase());
}

/** The tracked files worth re-ingesting, in a stable order. */
export function selectFiles(trackedPaths) {
  return trackedPaths
    .map((value) => value.trim())
    .filter(Boolean)
    .map((value) => value.replace(/\\/g, "/"))
    .filter((value) => !isIgnoredPath(value))
    .filter(isExtractable)
    .sort();
}

function parseArguments(argv) {
  const options = { dryRun: false, limit: Number.POSITIVE_INFINITY };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--dry-run") {
      options.dryRun = true;
    } else if (argument === "--limit") {
      const value = Number(argv[index + 1]);
      if (!Number.isFinite(value) || value <= 0) {
        throw new Error("--limit requires a positive number");
      }
      options.limit = value;
      index += 1;
    } else {
      throw new Error(`unknown argument: ${argument}`);
    }
  }
  return options;
}

function trackedFiles(workspace) {
  return execFileSync("git", ["ls-files"], {
    cwd: workspace,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  }).split(/\r?\n/);
}

async function main() {
  let options;
  try {
    options = parseArguments(process.argv.slice(2));
  } catch (error) {
    console.error(`reingest: ${error.message}`);
    process.exit(2);
  }

  const workspace = process.cwd();
  const files = selectFiles(trackedFiles(workspace)).slice(0, options.limit);

  if (options.dryRun) {
    console.log(`reingest: ${files.length} file(s) would be re-ingested`);
    for (const file of files) {
      console.log(`  ${file}`);
    }
    return;
  }

  // Build and drive our own server rather than reaching into whichever one an
  // editor happens to be running: a rebuilt binary does not change an already
  // running process, so an editor's server may still hold the old extractor —
  // which is the very thing this pass exists to get past.
  execFileSync("cargo", ["build", "-p", "mindleak-mcp"], {
    cwd: workspace,
    stdio: "inherit",
  });
  const executable = path.join(
    workspace,
    "target",
    "debug",
    process.platform === "win32" ? "mindleak-mcp.exe" : "mindleak-mcp",
  );

  const server = spawn(executable, [], {
    cwd: workspace,
    env: process.env,
    stdio: ["pipe", "pipe", "pipe"],
  });

  let nextId = 1;
  const pending = new Map();
  const sessionId = randomBytes(16).toString("hex");
  const lines = readline.createInterface({ input: server.stdout });
  lines.on("line", (line) => {
    const message = JSON.parse(line);
    const completion = pending.get(message.id);
    if (completion) {
      pending.delete(message.id);
      completion(message);
    }
  });

  const request = (method, params) => {
    const id = nextId++;
    server.stdin.write(
      `${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`,
    );
    return new Promise((resolve) => pending.set(id, resolve));
  };

  const callTool = async (name, arguments_) => {
    const response = await request("tools/call", {
      name,
      arguments: { ...arguments_, session_id: sessionId },
    });
    if (response.error || response.result?.isError) {
      throw new Error(JSON.stringify(response.error ?? response.result));
    }
    return JSON.parse(response.result.content[0].text);
  };

  const totals = { files: 0, nodes: 0, edges: 0, skipped: 0, failed: 0 };
  try {
    await request("initialize", {});
    await callTool("open_session", {});

    for (const file of files) {
      let content;
      try {
        content = fs.readFileSync(path.join(workspace, file), "utf8");
      } catch {
        // Unreadable or binary-in-disguise: skipped, and counted so the run
        // does not quietly report fewer files than it listed.
        totals.skipped += 1;
        continue;
      }
      try {
        const outcome = await callTool("ingest_file", { path: file, content });
        totals.files += 1;
        totals.nodes += outcome.nodes_created ?? 0;
        totals.edges += outcome.edges_created ?? 0;
      } catch (error) {
        totals.failed += 1;
        console.error(`reingest: ${file}: ${error.message}`);
      }
    }
  } finally {
    server.stdin.end();
    server.kill();
  }

  console.log(
    `reingest: ${totals.files} file(s) re-ingested, ` +
      `${totals.nodes} node(s) and ${totals.edges} edge(s) created, ` +
      `${totals.skipped} unreadable, ${totals.failed} failed.`,
  );
  if (totals.failed) {
    process.exit(1);
  }
}

// Only run the CLI when invoked directly, so the pure functions stay importable.
if (
  import.meta.url === `file://${process.argv[1]}` ||
  process.argv[1]?.endsWith("reingest.mjs")
) {
  await main();
}
