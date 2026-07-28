#!/usr/bin/env node
// Board health: separate the work a human must decide from the work nobody can.
//
// ADR-0058 decision 4 says the board should report what it cannot close. This
// is that report, and the distinction it draws is the whole point.
//
// `needs_human` is one verdict covering two unrelated situations:
//
//   DECIDABLE    conformance found something arguable -- drift, governed code
//                changed without a covering task, evidence that does not touch
//                the task's goal. A person reads it and rules. This is the
//                verdict working as designed.
//
//   UNRESOLVABLE the evidence bundle was empty, so there is nothing to rule on.
//                The work was never ingested; no amount of adjudication will
//                conjure it. Measured on this repository, this was 40 of 51
//                needs_human verdicts -- so the board reads as "51 decisions
//                pending" when four fifths of it is lost work wearing the same
//                label. That is worse than a backlog, because it makes the real
//                backlog invisible.
//
// The first measured run said something sharper than expected: of the 11
// genuinely decidable items, *all eleven* carried the same finding -- "evidence
// does not touch code bound to the task goal", which is ADR-0060's subject.
// So the board's fifty-one pending decisions contained no judgement calls at
// all. Forty were lost work, eleven were one rule producing a false negative,
// and zero needed a person to weigh anything. A report that could not tell
// those apart is why nobody looked.
//
// A third state is not a verdict at all: a claim whose lease has lapsed while
// the task still shows as claimed. It is neither in progress nor finished, and
// it holds scope against other agents (ADR-0048).
//
// Reporting only. Nothing here closes, abandons, or reassigns anything --
// ADR-0058 decision 5 is explicit that nothing closes automatically, and a
// report that mutated the board would be exactly the auto-closing this project
// has refused twice.

import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

/** The finding that means "there was nothing to judge", not "judge this". */
export const EMPTY_EVIDENCE = "no provenance-bearing mutation";

const findingsOf = (audit) => String(audit?.findings ?? "");

/** A claim that is still held but whose lease has run out (ADR-0048). */
export const isStrandedClaim = (task, now) =>
  task.status === "claimed" &&
  typeof task.lease_expires_at === "number" &&
  task.lease_expires_at < now;

/**
 * Split parked work by whether a person can actually act on it.
 *
 * `entries` is `[{ task, audit }]` where `audit` is the task's most recent
 * conformance audit, or undefined when it has never been audited.
 */
export function classify(entries, now) {
  const unresolvable = [];
  const decidable = [];
  const stranded = [];

  for (const entry of entries) {
    const { task, audit } = entry;
    if (isStrandedClaim(task, now)) stranded.push(entry);
    if (audit?.verdict !== "needs_human") continue;
    if (findingsOf(audit).includes(EMPTY_EVIDENCE)) unresolvable.push(entry);
    else decidable.push(entry);
  }
  return { unresolvable, decidable, stranded };
}

/** Render the report. Counts first, because the ratio is the finding. */
export function describe(report, entries) {
  const { unresolvable, decidable, stranded } = report;
  const parked = unresolvable.length + decidable.length;
  const lines = [
    `board: ${entries.length} tasks`,
    ``,
    `needs a human decision : ${decidable.length}`,
    `nobody can resolve     : ${unresolvable.length}   (evidence was empty -- the work was never ingested)`,
    `stranded claims        : ${stranded.length}   (lease lapsed, still held)`,
  ];
  if (parked > 0) {
    const share = Math.round((unresolvable.length / parked) * 100);
    lines.push(
      ``,
      `${share}% of parked work is unresolvable, not undecided. A board that` +
        ` reports those as pending decisions hides the ones that are.`,
    );
  }
  if (decidable.length > 0) {
    lines.push(``, `waiting on a person:`);
    for (const { task, audit } of decidable.slice(0, 10)) {
      lines.push(`  ${task.id}  ${findingsOf(audit).slice(0, 66)}`);
    }
  }
  if (stranded.length > 0) {
    lines.push(``, `stranded claims (hold scope against other agents):`);
    for (const { task } of stranded.slice(0, 10)) {
      lines.push(`  ${task.id}  ${String(task.title ?? "").slice(0, 56)}`);
    }
  }
  return lines.join("\n");
}

const client = (bin) => {
  const proc = spawn(bin, [], { stdio: ["pipe", "pipe", "pipe"] });
  const pending = new Map();
  let nextId = 1;
  createInterface({ input: proc.stdout }).on("line", (line) => {
    let m;
    try {
      m = JSON.parse(line);
    } catch {
      return;
    }
    const w = pending.get(m.id);
    if (!w) return;
    pending.delete(m.id);
    if (m.error) w.fail(new Error(m.error.message));
    else w.settle(m.result);
  });
  const send = (method, params) =>
    new Promise((settle, fail) => {
      const id = nextId++;
      pending.set(id, { settle, fail });
      proc.stdin.write(
        `${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`,
      );
    });
  const call = async (name, args) => {
    const r = await send("tools/call", { name, arguments: args });
    const text = r?.content?.[0]?.text;
    if (r?.isError) throw new Error(text ?? "tool error");
    return text ? JSON.parse(text) : null;
  };
  return { proc, send, call };
};

async function main() {
  const bin =
    process.env.LODESTAR_MCP_BIN ??
    (process.platform === "win32"
      ? "target/release/lodestar-mcp.exe"
      : "target/release/lodestar-mcp");
  const session = process.env.LODESTAR_SESSION_ID;
  if (!session) {
    console.error("board-health: set LODESTAR_SESSION_ID");
    process.exitCode = 2;
    return;
  }
  const { proc, send, call } = client(bin);
  await send("initialize", {
    protocolVersion: "2024-11-05",
    capabilities: {},
    clientInfo: { name: "board-health", version: "1" },
  });
  proc.stdin.write(
    `${JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized", params: {} })}\n`,
  );
  await call("open_session", { session_id: session });

  const board = await call("board", {});
  const tasks = Array.isArray(board) ? board : Object.values(board);
  const entries = [];
  for (const task of tasks) {
    let audit;
    try {
      const hist = await call("conformance_history", { task_id: task.id });
      const rows = Array.isArray(hist) ? hist : (hist?.audits ?? []);
      audit = rows[rows.length - 1];
    } catch {
      // A task with no history is not a problem; it simply has no verdict yet.
    }
    entries.push({ task, audit });
  }
  console.log(
    describe(classify(entries, Math.floor(Date.now() / 1000)), entries),
  );
  proc.kill();
}

if (
  process.argv[1] &&
  import.meta.url.endsWith(process.argv[1].replace(/\\/g, "/"))
) {
  main().catch((error) => {
    console.error(`board-health: ${error.message}`);
    process.exitCode = 1;
  });
}
