#!/usr/bin/env node
// Ingest a flat agent-memory Markdown file's entries into Lodestar's durable,
// revalidated knowledge store, instead of leaving them to accumulate in a
// file only the agent that wrote it ever rereads.
//
// This repository's own Memory Plane exists precisely so an agent does not
// need a private, ever-growing notes file: `record_knowledge` already
// carries provenance, reach (which future editors of a file see a lesson),
// and decay. A lesson that names the repository files it is about belongs
// there, surfacing to whoever next touches them, not sitting inert in
// Markdown. `source_ref` makes re-running this script idempotent: an
// unchanged entry reconfirms the same knowledge record rather than
// duplicating it (see `mcp_lodestar-mcp_record_knowledge`'s own doc comment).
//
// This does not attempt full automatic curation. `record_knowledge` refuses
// a `source_ref`-carrying call that names no artifact/symbol node or goal
// ("sourced knowledge must reference artifact/symbol nodes, a goal, or a
// known task"), so an entry with no repository file path in its body is
// never sent at all -- `--prune` only removes the entries that were actually
// ingested; everything else is left in the file rather than silently lost.

import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

import { callTools, resolveServer } from "./claim-gate.mjs";

/**
 * Split a memory file's Markdown into `##`-level entries.
 *
 * Pure: no file I/O, so the boundary logic is testable without a real
 * memory file. A leading preamble before the first `##` heading (this
 * repository's own memory files open with one, e.g. "# MindLeak repo —
 * workflow facts") is dropped -- it is prose framing, not a lesson to ingest.
 */
export function parseMemoryEntries(markdown) {
  const lines = markdown.split(/\r?\n/);
  const entries = [];
  let current = null;
  for (const line of lines) {
    const heading = /^##\s+(.+?)\s*$/.exec(line);
    if (heading) {
      if (current) entries.push(finalize(current));
      current = { heading: heading[1], bodyLines: [] };
      continue;
    }
    if (current) current.bodyLines.push(line);
  }
  if (current) entries.push(finalize(current));
  return entries;

  function finalize(entry) {
    const body = entry.bodyLines.join("\n").trim();
    return {
      heading: entry.heading,
      body,
      text: `## ${entry.heading}\n\n${body}`,
    };
  }
}

/**
 * A stable, filesystem-and-URL-safe slug for a heading.
 *
 * Used as the `source_ref` anchor, so the same entry re-ingested after only
 * a wording tweak elsewhere in the file still resolves to the same anchor
 * (the slug is derived from the heading, not the body).
 */
export function slugify(heading) {
  return heading
    .toLowerCase()
    .replace(/[`*]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 80);
}

const REPO_PATH_PATTERN =
  /`([A-Za-z0-9_.-]+(?:\/[A-Za-z0-9_.-]+)*\.(?:rs|mjs|cjs|ts|tsx|md|toml|ya?ml|json|sql|proto|ps1|sh))`/g;

/**
 * Repository-relative file paths named in an entry's body, order-preserved
 * and de-duplicated, capped at `maxNodes` so one entry that happens to list
 * many files does not turn into an oversized reach fan-out.
 *
 * Heuristic, not a repository file existence check: this only recognises a
 * backtick-quoted path shape already used throughout these memory files
 * (e.g. `` `crates/lodestar-core/src/store/mod.rs` ``). A path is kept only
 * when it contains a `/` (a bare root file like `` `Cargo.toml` `` is too
 * ambiguous with an unrelated code snippet to keep without more context).
 */
export function extractNodePaths(body, maxNodes = 6) {
  const found = [];
  const seen = new Set();
  for (const match of body.matchAll(REPO_PATH_PATTERN)) {
    const path = match[1];
    if (!path.includes("/") || seen.has(path)) continue;
    seen.add(path);
    found.push(path);
    if (found.length >= maxNodes) break;
  }
  return found;
}

/**
 * The `record_knowledge` arguments for one parsed entry, or `null` when the
 * entry is too short to be a real lesson, or names no repository file this
 * script can find a reach for.
 *
 * `record_knowledge` refuses a call that carries `source_ref` but neither
 * `nodes` nor a `goal` ("sourced knowledge must reference artifact/symbol
 * nodes, a goal, or a known task") -- discovered by actually running this
 * against the live server, not from the tool's own doc comment, which reads
 * as if an unreached record is merely inert ("arrives nowhere") rather than
 * refused outright. Since every call here sets `source_ref` (so a re-run
 * reconfirms the same record instead of duplicating it), an entry with no
 * extractable path is left in the source file rather than sent at all --
 * that is this function's contribution to curation, not just ingestion.
 *
 * `evidence` is JSON-stringified because the tool's own schema declares it a
 * string, not an object -- true of every raw JSON-RPC caller in this
 * repository's scripts, unlike the editor's tool-call surface which accepts
 * an object and serialises it for you.
 */
export function knowledgeArgsFor(entry, memoryFilePath, sessionId) {
  if (entry.body.length < 20) return null;
  const nodes = extractNodePaths(entry.body);
  if (nodes.length === 0) return null;
  const evidence = {
    method:
      "migrated from a local agent memory file (scripts/ingest-agent-memory.mjs)",
    nodes: nodes.map((path) => `artifact:${path}`),
  };
  return {
    session_id: sessionId,
    statement: entry.text,
    evidence: JSON.stringify(evidence),
    source_ref: `${memoryFilePath}#${slugify(entry.heading)}`,
  };
}

/** Split an array into chunks of at most `size`, preserving order. */
function chunk(items, size) {
  const chunks = [];
  for (let index = 0; index < items.length; index += size) {
    chunks.push(items.slice(index, index + size));
  }
  return chunks;
}

async function main() {
  const args = process.argv.slice(2);
  const dryRun = args.includes("--dry-run");
  const prune = args.includes("--prune");
  const fileArg = args.find((arg) => !arg.startsWith("--"));
  if (!fileArg) {
    console.error(
      "usage: node scripts/ingest-agent-memory.mjs <memory-file> [--dry-run] [--prune]",
    );
    process.exitCode = 1;
    return;
  }
  const memoryFilePath = fileArg;
  const virtualPath = args[args.indexOf(fileArg) + 1]?.startsWith("--")
    ? null
    : args[args.indexOf(fileArg) + 1];
  const sourceRefBase = virtualPath ?? "/memories/repo/mindleak-workflow.md";

  const markdown = readFileSync(memoryFilePath, "utf8");
  const entries = parseMemoryEntries(markdown);
  const candidates = entries.filter(
    (entry) =>
      entry.body.length >= 20 && extractNodePaths(entry.body).length > 0,
  );

  console.log(
    `ingest-agent-memory: ${entries.length} entries parsed, ${candidates.length} name a repository file this tool can reach`,
  );

  if (dryRun) {
    for (const entry of candidates.slice(0, 5)) {
      const nodes = extractNodePaths(entry.body);
      console.log(
        `  - ${entry.heading.slice(0, 70)} (${nodes.length} node${nodes.length === 1 ? "" : "s"})`,
      );
    }
    if (candidates.length > 5)
      console.log(`  ... and ${candidates.length - 5} more`);
    return;
  }

  const sessionId = process.env.LODESTAR_SESSION_ID;
  if (!sessionId || !/^[0-9a-f]{32}$/.test(sessionId)) {
    console.error(
      "ingest-agent-memory: LODESTAR_SESSION_ID must be a registered 32-hex session id",
    );
    process.exitCode = 1;
    return;
  }
  const repoRoot = process.cwd();
  const server = resolveServer(repoRoot, "lodestar");
  if (!server) {
    console.error(
      "ingest-agent-memory: no lodestar-mcp binary found (build one, or set LODESTAR_MCP_BIN)",
    );
    process.exitCode = 1;
    return;
  }

  const reached = new Set();
  const failed = [];
  for (const batch of chunk(candidates, 20)) {
    const results = callTools(server, repoRoot, [
      { name: "open_session", arguments: { session_id: sessionId } },
      ...batch.map((entry) => ({
        name: "record_knowledge",
        arguments: knowledgeArgsFor(entry, sourceRefBase, sessionId),
      })),
    ]);
    // results[0] is open_session's own reply; one record_knowledge reply follows per batch entry.
    for (let index = 0; index < batch.length; index += 1) {
      const result = results[index + 1];
      const heading = batch[index].heading;
      // A successful record_knowledge reply is an object carrying `reach`; a
      // refused call's reply is a plain human-readable string (see
      // parseCallResult's JSON.parse fallback) -- conflating the two here
      // once made a refused write print as if it had merely landed with no
      // reach, silently hiding that nothing was recorded at all.
      if (result && typeof result === "object" && "reach" in result) {
        reached.add(heading);
        console.log(`  ingested (${result.reach}): ${heading.slice(0, 70)}`);
      } else {
        failed.push({ heading, result });
        console.log(`  FAILED: ${heading.slice(0, 70)} -- ${result}`);
      }
    }
  }
  console.log(
    `ingest-agent-memory: ${reached.size} of ${candidates.length} ingested, ${failed.length} failed`,
  );

  if (prune && reached.size > 0) {
    const remaining = entries.filter((entry) => !reached.has(entry.heading));
    const preamble = markdown.slice(0, markdown.indexOf("\n## "));
    const body = remaining.map((entry) => entry.text).join("\n\n");
    writeFileSync(memoryFilePath, `${preamble}\n${body}\n`, "utf8");
    console.log(
      `ingest-agent-memory: pruned ${reached.size} ingested entries from ${memoryFilePath}`,
    );
  }
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  main();
}
