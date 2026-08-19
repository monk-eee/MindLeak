// Tests for scripts/status.mjs. Run with: make script-test
//
// This exists so a person can read live Lodestar/MindLeak state without an
// agent relaying MCP tool output. These tests cover the pure aggregation and
// formatting only -- the real reads (resolveServer, callTools against a
// spawned server binary) are exercised by hand against a live checkout, the
// same way delivery-queue.mjs's own git-calling helpers are.
import assert from "node:assert/strict";
import { test } from "node:test";

import { formatReport, liveClaims } from "./status.mjs";

const NOW = 1_000_000;

const task = (id, status, over = {}) => ({
  id,
  title: `task ${id}`,
  status,
  owner: null,
  lease_expires_at: null,
  ...over,
});

// --- liveClaims -------------------------------------------------------------

test("terminal tasks (done, abandoned) are excluded", () => {
  const board = [
    task("t1", "done"),
    task("t2", "abandoned"),
    task("t3", "open"),
  ];
  assert.deepEqual(
    liveClaims(board, NOW).map((c) => c.id),
    ["t3"],
  );
});

test("a claimed task with a live lease is not flagged as lapsed", () => {
  const board = [
    task("t1", "claimed", {
      owner: "session:v1:abc",
      lease_expires_at: NOW + 600,
    }),
  ];
  const [claim] = liveClaims(board, NOW);
  assert.equal(claim.owner, "session:v1:abc");
  assert.equal(claim.lapsed, false);
});

test("a claimed task whose lease already expired is flagged as lapsed", () => {
  const board = [
    task("t1", "claimed", {
      owner: "session:v1:abc",
      lease_expires_at: NOW - 1,
    }),
  ];
  const [claim] = liveClaims(board, NOW);
  assert.equal(claim.lapsed, true);
});

/// A blocked or in_review task is live (worth showing) but was never claimed,
/// so it must never read as lapsed just because it has no lease at all.
test("a non-claimed live task (blocked, in_review) is never reported as lapsed", () => {
  const board = [task("t1", "blocked"), task("t2", "in_review")];
  const claims = liveClaims(board, NOW);
  assert.equal(
    claims.every((c) => c.lapsed === false),
    true,
  );
});

// --- formatReport -----------------------------------------------------------

const baseInput = () => ({
  lodestarStats: {
    active_goals: 5,
    open_tasks: 1,
    claimed_tasks: 2,
    done_tasks: 100,
    active_knowledge: 40,
  },
  doctor: [],
  claims: [],
  graph: null,
  telemetry: null,
  now: NOW,
});

test("the report names every lodestar_stats figure", () => {
  const rendered = formatReport(baseInput());
  assert.match(rendered, /5 active goals/);
  assert.match(rendered, /1 open/);
  assert.match(rendered, /2 claimed/);
  assert.match(rendered, /100 done/);
  assert.match(rendered, /40 active knowledge/);
});

test("a clean board says so explicitly rather than printing nothing", () => {
  const rendered = formatReport(baseInput());
  assert.match(rendered, /no board ailments/);
  assert.match(rendered, /nothing open, claimed, blocked, or in review/);
});

/// The doctor finding's own remedy is the whole point of the view (ADR-0058
/// decision 5: it judges nothing, it only names what a person could do) --
/// the report must carry that text through, not just the ailment tag.
test("a doctor finding names its ailment, subject, and remedy", () => {
  const input = baseInput();
  input.doctor = [
    {
      ailment: "duplicate_title",
      task_ids: ["task:a", "task:b"],
      subject: "Implement: ADR-0090",
      remedy: "abandon task:b, the later duplicate",
    },
  ];
  const rendered = formatReport(input);
  assert.match(rendered, /\[duplicate_title\]/);
  assert.match(rendered, /Implement: ADR-0090/);
  assert.match(rendered, /task:a, task:b/);
  assert.match(rendered, /abandon task:b, the later duplicate/);
});

test("a lapsed claim is called out, an ordinary one is not", () => {
  const input = baseInput();
  input.claims = [
    {
      id: "task:1",
      title: "fine",
      status: "claimed",
      owner: "session:v1:abc",
      lapsed: false,
    },
    {
      id: "task:2",
      title: "stuck",
      status: "claimed",
      owner: "session:v1:def",
      lapsed: true,
    },
  ];
  const rendered = formatReport(input);
  assert.match(rendered, /task:1\s+claimed by session:v1:abc\s+fine/);
  assert.match(
    rendered,
    /task:2\s+claimed by session:v1:def -- LEASE LAPSED\s+stuck/,
  );
});

test("a non-claimed live task is shown by its own status, not as a claim", () => {
  const input = baseInput();
  input.claims = [
    {
      id: "task:1",
      title: "waiting",
      status: "blocked",
      owner: null,
      lapsed: false,
    },
  ];
  const rendered = formatReport(input);
  assert.match(rendered, /task:1\s+blocked\s+waiting/);
});

/// A checkout with no mindleak-mcp binary built yet still has a Lodestar half
/// worth reporting -- the report must say what it can, not fail outright or
/// print an empty section for the plane it could not reach.
test("a missing graph or telemetry section is omitted, not printed empty", () => {
  const rendered = formatReport(baseInput());
  assert.equal(rendered.includes("MindLeak graph:"), false);
  assert.equal(rendered.includes("MindLeak telemetry:"), false);
});

test("graph and telemetry sections appear once their data is supplied", () => {
  const input = baseInput();
  input.graph = {
    nodes: 10,
    active_edges: 20,
    unembedded_nodes: 1,
    split_identity_nodes: 0,
  };
  input.telemetry = { total_events: 500, currently_failing_tools: 2 };
  const rendered = formatReport(input);
  assert.match(rendered, /10 nodes, 20 active edges/);
  assert.match(rendered, /2 tool\(s\) currently failing/);
});
