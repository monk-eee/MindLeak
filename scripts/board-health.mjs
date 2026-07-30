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

import { spawn, execFileSync } from "node:child_process";
import { createInterface } from "node:readline";

import { resolveServer } from "./claim-gate.mjs";

const repoRoot = execFileSync("git", ["rev-parse", "--show-toplevel"], {
  encoding: "utf8",
}).trim();

/** The finding that means "there was nothing to judge", not "judge this". */
export const EMPTY_EVIDENCE = "no provenance-bearing mutation";

/**
 * A task that has already reached the end of its life. Its audits are history,
 * not a queue.
 *
 * Leaving these in was the first version's mistake, and it is worth naming
 * because it is easy to repeat: a task keeps its conformance audits after it
 * completes, so classifying by "latest audit" alone counts finished work as
 * pending. The first run reported 51 parked tasks; every one of them was
 * already `done` or `abandoned`, and the real figure was zero. A report that
 * inflates the backlog is not a smaller version of a useful report -- it sends
 * people looking for work that does not exist, which is the same disease as
 * the verdict it was written to untangle.
 */
export const TERMINAL = new Set(["done", "abandoned"]);
export const isLive = (task) => !TERMINAL.has(task.status);

const findingsOf = (audit) => String(audit?.findings ?? "");

/** A claim that is still held but whose lease has run out (ADR-0048).
 *
 * Calling these "stranded" invited the obvious response -- have an agent pick
 * them up and close them -- and that response cannot work. Closing one requires
 * re-claiming it, and re-claiming after a lapse records the lapse, whereupon
 * conformance returns `needs_human` for a discontinuous evidence window and
 * refuses to certify across the hole. That is deliberate: narrowing the window
 * around the gap is precisely the laundering ADR-0048 exists to stop, so the
 * refusal is the guarantee working, not a defect to route around.
 *
 * Measured while trying: a task showing `0 lapse(s)` reported
 * `the lease lapsed 1 time(s), leaving 85730s unleased` immediately after being
 * claimed in order to close it. Acquiring the claim is what creates the hole.
 * Three tasks were moved to `in_review` learning this. The label now says what
 * is actually true -- a person must confirm these -- rather than implying work
 * an agent could pick up.
 */
export const isStrandedClaim = (task, now) =>
  task.status === "claimed" &&
  typeof task.lease_expires_at === "number" &&
  task.lease_expires_at < now;

/**
 * Which branches have landed on `main`, and in which merge commit.
 *
 * Parsed from merge subjects rather than `git branch --merged`, because a
 * branch is usually deleted the moment it merges — the ref is gone while the
 * history that proves it landed is not. Pure: it takes the lines so the parsing
 * is testable without a repository.
 *
 * `lines` are `"<sha> <subject>"` from `git log --merges`.
 */
export function mergedBranches(lines) {
  const merged = new Map();
  for (const line of lines) {
    const [sha, ...rest] = String(line).trim().split(/\s+/);
    const subject = rest.join(" ");
    // "Merge pull request #186 from monk-eee/docs/the-blind-spot-is-recorded"
    const match = subject.match(/^Merge pull request #\d+ from [^/\s]+\/(\S+)/);
    if (!match || !sha) continue;
    // First wins: `git log` is newest-first, and a branch name reused after a
    // delete should resolve to the merge that most recently landed it.
    if (!merged.has(match[1])) merged.set(match[1], sha);
  }
  return merged;
}

/**
 * Work that shipped and never closed.
 *
 * The board understates what is finished, and that is expensive in a way an
 * overstated board is not: `next_task` offers work that already exists, so an
 * agent rebuilds it. Observed repeatedly — a task was offered whose branch was
 * sitting in an open pull request, and four separate open tasks turned out to be
 * already delivered.
 *
 * Reports and never closes. Completing one of these would manufacture a receipt
 * for work this script did not witness, which ADR-0009 refuses; the merge commit
 * is named so a person can check it in seconds.
 */
export const shippedButOpen = (task, merged) =>
  isLive(task) && typeof task.branch === "string" && merged.has(task.branch);

/**
 * Split parked work by whether a person can actually act on it.
 *
 * `entries` is `[{ task, audit }]` where `audit` is the task's most recent
 * conformance audit, or undefined when it has never been audited. Terminal
 * tasks are excluded: their verdicts are a record of what happened, not a
 * request for anyone to do anything.
 */
export function classify(entries, now, merged = new Map()) {
  const unresolvable = [];
  const decidable = [];
  const stranded = [];
  const shipped = [];

  for (const entry of entries) {
    const { task, audit } = entry;
    if (!isLive(task)) continue;
    if (isStrandedClaim(task, now)) stranded.push(entry);
    if (shippedButOpen(task, merged)) {
      shipped.push({ ...entry, mergedAt: merged.get(task.branch) });
    }
    if (audit?.verdict !== "needs_human") continue;
    if (findingsOf(audit).includes(EMPTY_EVIDENCE)) unresolvable.push(entry);
    else decidable.push(entry);
  }
  return { unresolvable, decidable, stranded, shipped };
}

/** Render the report. Counts first, because the ratio is the finding. */
export function describe(report, entries) {
  const { unresolvable, decidable, stranded, shipped = [] } = report;
  const parked = unresolvable.length + decidable.length;
  // Zero because nothing shipped unclosed, or zero because nothing records a
  // branch to check? Those read identically and mean opposite things. A task
  // claimed before the branch column existed records none, and a server built
  // before it does not return the column at all — so a bare 0 here would be the
  // same falsely-reassuring signal this report exists to remove.
  const recorded = entries.filter(
    ({ task }) => typeof task.branch === "string" && task.branch.length > 0,
  ).length;
  const shippedLine =
    recorded === 0
      ? `shipped, never closed  : unknown   (no task records a branch; nothing to check against)`
      : `shipped, never closed  : ${shipped.length}   (its branch is on main; the board understates what is done)`;
  const lines = [
    `board: ${entries.length} tasks`,
    ``,
    `needs a human decision : ${decidable.length}`,
    `nobody can resolve     : ${unresolvable.length}   (evidence was empty -- the work was never ingested)`,
    `awaiting confirmation  : ${stranded.length}   (lapsed claim; only a human can close it -- see below)`,
    shippedLine,
  ];
  // Only worth saying when there is something to say. "0% of parked work is
  // unresolvable" is a sentence about nothing, and a report that pads itself
  // teaches readers to skim past the lines that matter.
  if (parked > 0 && unresolvable.length > 0) {
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
    lines.push(
      ``,
      `lapsed claims a person must confirm (ADR-0048 -- an agent cannot close these:`,
      `closing one means re-claiming it, and re-claiming records the lapse the rule`,
      `refuses to certify across). \`make stranded-report\` names the likely commit:`,
    );
    for (const { task } of stranded.slice(0, 10)) {
      lines.push(`  ${task.id}  ${String(task.title ?? "").slice(0, 56)}`);
    }
  }
  if (shipped.length > 0) {
    lines.push(
      ``,
      `shipped but still on the board. The branch each names has merged into main,`,
      `so next_task can offer work that already exists and an agent rebuilds it.`,
      `Reported, never closed: completing one here would manufacture a receipt for`,
      `work this script did not witness (ADR-0009). The merge commit is named so a`,
      `person can check it in seconds:`,
    );
    for (const { task, mergedAt } of shipped.slice(0, 10)) {
      lines.push(
        `  ${task.id}  ${String(mergedAt).slice(0, 8)}  ${String(task.title ?? "").slice(0, 48)}`,
      );
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
  // Resolved through the shared helper, which honours the override, accepts a
  // debug build, and returns null rather than handing back a path that is not
  // there. Forking this logic release-only meant the report crashed with an
  // unhandled ENOENT for anyone who had not run a release build, which is the
  // normal state - so a report about the board's health could not be run by
  // most of the people it was written for.
  const bin = resolveServer(repoRoot, "lodestar");
  if (!bin) {
    console.error(
      "board-health: no lodestar-mcp binary found.\n" +
        "  Build one:  cargo build -p lodestar-mcp\n" +
        "  Or point at one:  set LODESTAR_MCP_BIN",
    );
    process.exitCode = 2;
    return;
  }
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

  const board = await call("task_query", { view: "board" });
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
  // Which branches have landed. `git log` on the tracking ref rather than a
  // network call: the report stays usable offline and costs nothing.
  let merged = new Map();
  try {
    merged = mergedBranches(
      execFileSync(
        "git",
        ["log", "origin/main", "--merges", "--format=%H %s", "-n", "500"],
        {
          cwd: repoRoot,
          encoding: "utf8",
        },
      ).split(/\r?\n/),
    );
  } catch {
    // No origin/main here (a fresh clone, a detached CI checkout). Everything
    // else in the report still stands, so say nothing and carry on.
  }
  console.log(
    describe(classify(entries, Math.floor(Date.now() / 1000), merged), entries),
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
