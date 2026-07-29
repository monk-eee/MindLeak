#!/usr/bin/env node
// Prepare stranded claims for the human confirmation they require.
//
// Every stranded claim has a discontinuous evidence window, and conformance
// returns needs_human for that unconditionally -- deliberately, because letting
// an agent narrow its evidence window around the hole is the laundering ADR-0048
// exists to stop. So no agent can close these, however good its evidence.
//
// Continuity is derived from the task log and rides on each board row as
// `claim_window` (ADR-0064 d5); it used to be the `claim_lapses` and
// `unleased_seconds` columns on the task itself.
//
// What an agent *can* do is remove the investigation. For each claim this finds
// the commit that most likely shipped the work and how long the lease has been
// gone, so confirming becomes a judgement about one named commit rather than an
// archaeology exercise across a day of fleet history.
//
// It proposes; it never decides. The match is a similarity score over words,
// and it is reported with its confidence so a weak guess reads as a weak guess.

import { spawn, execFileSync } from "node:child_process";
import { createInterface } from "node:readline";

import { resolveServer } from "./claim-gate.mjs";

const repoRoot = execFileSync("git", ["rev-parse", "--show-toplevel"], {
  encoding: "utf8",
}).trim();

/** Words too common in this repository to carry any signal. */
const STOP = new Set([
  "the",
  "a",
  "an",
  "is",
  "are",
  "be",
  "must",
  "not",
  "and",
  "or",
  "of",
  "to",
  "in",
  "on",
  "for",
  "it",
  "its",
  "that",
  "this",
  "with",
  "from",
  "into",
  "by",
  "at",
  "as",
  "but",
  "can",
  "cannot",
  "does",
  "do",
  "when",
  "what",
  "which",
  "every",
  "any",
  "no",
  "never",
  "always",
  "only",
  "than",
  "then",
  "so",
  "record",
  "fix",
  "make",
  "add",
  "give",
  "let",
  "one",
  "two",
]);

const words = (text) =>
  new Set(
    String(text)
      .toLowerCase()
      .split(/[^a-z0-9_]+/)
      .filter((w) => w.length > 2 && !STOP.has(w)),
  );

/**
 * How strongly a commit subject matches a task title, as the share of the
 * task's distinctive words the subject also uses. Jaccard would punish long
 * commit subjects for being descriptive, which is the opposite of useful here.
 */
export function similarity(title, subject) {
  const want = words(title);
  if (want.size === 0) return 0;
  const have = words(subject);
  let hit = 0;
  for (const w of want) if (have.has(w)) hit += 1;
  return hit / want.size;
}

/** Pick the best-matching commit, with the runner-up to expose ambiguity. */
export function bestMatch(title, commits) {
  const scored = commits
    .map((c) => ({ ...c, score: similarity(title, c.subject) }))
    .sort((a, b) => b.score - a.score);
  return { best: scored[0], next: scored[1] };
}

/** How a score should be described to someone deciding whether to trust it. */
export function confidence(best, next) {
  if (!best || best.score < 0.3) return "none";
  const margin = best.score - (next?.score ?? 0);
  if (best.score >= 0.6 && margin >= 0.2) return "strong";
  if (best.score >= 0.45) return "likely";
  return "weak";
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
    w(m.result);
  });
  const send = (method, params) =>
    new Promise((settle) => {
      const id = nextId++;
      pending.set(id, settle);
      proc.stdin.write(
        `${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`,
      );
    });
  const call = async (name, args) => {
    const r = await send("tools/call", { name, arguments: args });
    const text = r?.content?.[0]?.text;
    return text ? JSON.parse(text) : null;
  };
  return { proc, send, call };
};

const hours = (secs) => {
  const h = Math.round(secs / 3600);
  return h >= 48 ? `${Math.round(h / 24)}d` : `${h}h`;
};

async function main() {
  // Shared resolver: honours the override, accepts a debug build, and reports
  // absence instead of spawning a path that does not exist. A release-only
  // fork made this report unrunnable for anyone without a release build.
  const bin = resolveServer(repoRoot, "lodestar");
  if (!bin) {
    console.error(
      "stranded-report: no lodestar-mcp binary found.\n" +
        "  Build one:  cargo build -p lodestar-mcp\n" +
        "  Or point at one:  set LODESTAR_MCP_BIN",
    );
    process.exitCode = 2;
    return;
  }
  const session = process.env.LODESTAR_SESSION_ID;
  if (!session) {
    console.error("stranded-report: set LODESTAR_SESSION_ID");
    process.exitCode = 2;
    return;
  }

  const commits = execFileSync(
    "git",
    [
      "log",
      "origin/main",
      "--no-merges",
      "--since=7.days",
      "--format=%h%x00%ct%x00%s",
    ],
    { encoding: "utf8", maxBuffer: 1 << 26 },
  )
    .trim()
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => {
      const [sha, at, subject] = line.split("\0");
      return { sha, at: Number(at), subject };
    });

  const { proc, send, call } = client(bin);
  await send("initialize", {
    protocolVersion: "2024-11-05",
    capabilities: {},
    clientInfo: { name: "stranded-report", version: "1" },
  });
  proc.stdin.write(
    `${JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized", params: {} })}\n`,
  );
  await call("open_session", { session_id: session });

  const board = await call("board", {});
  const now = Math.floor(Date.now() / 1000);
  const stranded = (Array.isArray(board) ? board : Object.values(board)).filter(
    (t) => t.status === "claimed" && (t.lease_expires_at ?? 0) < now,
  );

  console.log(
    `${stranded.length} claims need a human to confirm a lapsed window.`,
  );
  console.log(
    `Conformance refuses these on its own authority (ADR-0048): the lease lapsed, so the`,
  );
  console.log(
    `evidence window has a hole, and no agent may certify across it.\n`,
  );

  const buckets = { strong: [], likely: [], weak: [], none: [] };
  for (const task of stranded) {
    const { best, next } = bestMatch(task.title, commits);
    buckets[confidence(best, next)].push({ task, best });
  }

  for (const level of ["strong", "likely", "weak", "none"]) {
    const rows = buckets[level];
    if (rows.length === 0) continue;
    console.log(`--- ${level} match (${rows.length}) ---`);
    for (const { task, best } of rows) {
      console.log(`${task.id}  ${String(task.title).slice(0, 66)}`);
      console.log(
        `    lapsed ${hours(now - (task.lease_expires_at ?? now))} ago, ` +
          `${task.claim_window?.lapses ?? 0} lapse(s), ` +
          `${hours(task.claim_window?.unleased_seconds ?? 0)} unleased`,
      );
      if (level === "none")
        console.log(
          `    no commit resembles this; it may be genuinely unfinished`,
        );
      else
        console.log(
          `    shipped by?  ${best.sha}  ${best.subject.slice(0, 62)}`,
        );
    }
    console.log("");
  }
  proc.kill();
}

if (
  process.argv[1] &&
  import.meta.url.endsWith(process.argv[1].replace(/\\/g, "/"))
) {
  main().catch((error) => {
    console.error(`stranded-report: ${error.message}`);
    process.exitCode = 1;
  });
}
