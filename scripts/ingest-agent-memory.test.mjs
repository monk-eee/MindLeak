// Tests for the agent-memory-to-Lodestar ingestion parser. Run with: make script-test
//
// The regression these lock down: `record_knowledge` refuses a
// `source_ref`-carrying call that names no artifact/symbol node or goal, so
// an entry with no repository file path in its body must never be sent --
// `knowledgeArgsFor` returns `null` for it rather than building a call that
// would fail against the live server.
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  extractNodePaths,
  knowledgeArgsFor,
  parseMemoryEntries,
  slugify,
} from "./ingest-agent-memory.mjs";

const SESSION = "c1a8f273b95e4d67a0c214e89f36ab50";

test("parseMemoryEntries splits on level-2 headings and drops the preamble", () => {
  const markdown = [
    "# MindLeak repo — workflow facts",
    "",
    "Some preamble prose nobody should ingest.",
    "",
    "## First lesson",
    "- detail one",
    "- detail two",
    "",
    "## Second lesson",
    "- another detail",
  ].join("\n");

  const entries = parseMemoryEntries(markdown);

  assert.equal(entries.length, 2);
  assert.equal(entries[0].heading, "First lesson");
  assert.equal(entries[0].body, "- detail one\n- detail two");
  assert.equal(entries[1].heading, "Second lesson");
  assert.equal(entries[1].body, "- another detail");
});

test("parseMemoryEntries returns nothing for a file with no ## headings", () => {
  assert.deepEqual(parseMemoryEntries("just prose, no headings"), []);
});

test("parseMemoryEntries keeps the last entry when the file does not end with a blank line", () => {
  const entries = parseMemoryEntries(
    "## Only entry\n- one line, no trailing newline",
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0].heading, "Only entry");
});

test("slugify produces a stable, filesystem-safe anchor", () => {
  assert.equal(
    slugify('RELAPSE: task_claim(step="renew") without lease_secs'),
    "relapse-task-claim-step-renew-without-lease-secs",
  );
});

test("slugify strips markdown emphasis characters", () => {
  assert.equal(slugify("`code` and *emphasis*"), "code-and-emphasis");
});

test("extractNodePaths finds every distinct backtick-quoted repository path", () => {
  const body =
    "Fixed in `crates/lodestar-core/src/store/mod.rs` and again in " +
    "`crates/lodestar-mcp/src/tools/lifecycle.rs`. Reused `crates/lodestar-core/src/store/mod.rs`.";

  const paths = extractNodePaths(body);

  assert.deepEqual(paths, [
    "crates/lodestar-core/src/store/mod.rs",
    "crates/lodestar-mcp/src/tools/lifecycle.rs",
  ]);
});

test("extractNodePaths ignores a bare root filename with no directory", () => {
  // `Cargo.toml` alone is too ambiguous with an unrelated inline snippet to
  // treat as a real reach target without a directory qualifying it.
  assert.deepEqual(extractNodePaths("See `Cargo.toml` for the version."), []);
});

test("extractNodePaths caps the number of paths it returns", () => {
  const body = Array.from(
    { length: 10 },
    (_, index) => `\`crates/a/file${index}.rs\``,
  ).join(" ");

  assert.equal(extractNodePaths(body, 3).length, 3);
});

test("extractNodePaths returns nothing for prose with no file paths", () => {
  assert.deepEqual(
    extractNodePaths("A general lesson about PowerShell pipes."),
    [],
  );
});

test("knowledgeArgsFor declares nodes when the entry names repository files", () => {
  const entry = {
    heading: "A real bug",
    body: "Fixed in `crates/lodestar-core/src/store/mod.rs`.",
    text: "## A real bug\n\nFixed in `crates/lodestar-core/src/store/mod.rs`.",
  };

  const args = knowledgeArgsFor(
    entry,
    "/memories/repo/mindleak-workflow.md",
    SESSION,
  );

  assert.equal(args.session_id, SESSION);
  assert.equal(args.statement, entry.text);
  assert.equal(
    args.source_ref,
    "/memories/repo/mindleak-workflow.md#a-real-bug",
  );
  const evidence = JSON.parse(args.evidence);
  assert.deepEqual(evidence.nodes, [
    "artifact:crates/lodestar-core/src/store/mod.rs",
  ]);
});

test("knowledgeArgsFor returns null when the entry names no repository file", () => {
  // record_knowledge refuses a source_ref-carrying call with no nodes/goal
  // ("sourced knowledge must reference artifact/symbol nodes, a goal, or a
  // known task") -- sending one anyway would just fail against the live
  // server, discovered only by actually running this tool for real.
  const entry = {
    heading: "A generic tool-usage lesson",
    body: "PowerShell pipes corrupt encoding; never use them.",
    text: "## A generic tool-usage lesson\n\nPowerShell pipes corrupt encoding; never use them.",
  };

  assert.equal(
    knowledgeArgsFor(entry, "/memories/repo/mindleak-workflow.md", SESSION),
    null,
  );
});

test("knowledgeArgsFor returns null for a heading with no real body", () => {
  const entry = {
    heading: "Stray heading",
    body: "",
    text: "## Stray heading\n\n",
  };

  assert.equal(
    knowledgeArgsFor(entry, "/memories/repo/mindleak-workflow.md", SESSION),
    null,
  );
});
