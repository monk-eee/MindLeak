// Known gaps as per-gap fragments (the ADR-0056 treatment, applied to gaps).
//
// The Known gaps section of DEVELOPERS.md was one shared append-only list, so
// every branch that recorded a gap edited the same lines and every merge
// collided there — hand-resolved four times in a single session, each time
// producing a conflict that expressed no disagreement at all: two agents adding
// two unrelated observations to the same paragraph.
//
// ADR-0056 already solved this shape for CHANGELOG.md. A fragment is a new file
// per item, and two branches never write the same path.
//
// ONE DELIBERATE DIFFERENCE FROM changelog.d. A changelog fragment is temporary:
// `--release` folds it into CHANGELOG.md and deletes it. A gap has no release
// event — it is open until it is fixed — so folding would put the shared list
// straight back and the conflict with it. The fragments are therefore the
// source of truth, permanently, and DEVELOPERS.md points at them rather than
// holding a generated copy. Closing a gap deletes its fragment, which is
// attributable in the commit that fixes it.
//
// Platform-agnostic: node only. Usage:
//   node scripts/gaps.mjs --check     validate fragments (hook/CI)
//   node scripts/gaps.mjs --list      print every open gap, for reading
//   node scripts/gaps.mjs --triage    reliability scorecard: backlog age + task linkage

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { callTools, resolveServer } from "./claim-gate.mjs";

export const FRAGMENT_DIR = "gaps.d";

/** A fragment name is the gap's slug: lowercase, kebab, `.md`. */
export const isFragmentName = (name) =>
  /^[a-z0-9]+(-[a-z0-9]+)*\.md$/.test(name);

const terminalStatusWithoutOpenResidual = (heading) => {
  const status = heading.split(/\s+(?:—|--)\s+/).at(-1) ?? "";
  return (
    /\b(?:FIXED|RESOLVED|CLOSED)\b/i.test(status) && !/\bOPEN\b/i.test(status)
  );
};

/**
 * Read every fragment, newest-agnostic and sorted by name so the rendered order
 * is stable. Unreadable or malformed fragments are collected rather than thrown,
 * so `--check` can report all of them at once instead of one per run.
 */
export const readFragments = (dir = FRAGMENT_DIR) => {
  if (!existsSync(dir)) return { gaps: [], files: [], problems: [] };
  const files = readdirSync(dir)
    .filter((name) => name.endsWith(".md") && name !== "README.md")
    .sort();

  const gaps = [];
  const problems = [];
  for (const name of files) {
    if (!isFragmentName(name)) {
      problems.push(
        `${name}: name must be <slug>.md, lowercase and kebab-case`,
      );
      continue;
    }
    const body = readFileSync(join(dir, name), "utf8").replace(/\s+$/, "");
    const heading = body.match(/^- \*\*([\s\S]*?)\*\*/)?.[1];
    if (!heading) {
      problems.push(
        `${name}: must open with a "- **" bullet naming the gap and its status`,
      );
      continue;
    }
    if (terminalStatusWithoutOpenResidual(heading)) {
      problems.push(
        `${name}: terminal status has no OPEN residual; delete the fragment when the gap closes`,
      );
      continue;
    }
    gaps.push({ name, body });
  }
  return { gaps, files, problems };
};

/** Every open gap, as one markdown list. */
export const render = (gaps) => gaps.map((gap) => gap.body).join("\n\n");

// --- Reliability scorecard: backlog age + task linkage --------------------
//
// A gap fragment records that something is broken. It does not, by itself,
// record whether anyone is fixing it -- a fragment can sit in this directory
// indefinitely and look exactly as "handled" as one filed five minutes ago.
// `--triage` answers the two questions that distinguish them: how long has
// this been open, and is a Lodestar task tracking its fix.

/**
 * A tracking link is metadata, not any historical task id mentioned in prose.
 * Keeping it on its own line makes the task a deliberate current commitment
 * rather than an incident reference that happened to look like one.
 */
export const TRACKING_TASK_PATTERN = /^Tracking:\s*(task:[0-9a-f]{12})\s*$/im;

export const trackingTaskId = (body) =>
  TRACKING_TASK_PATTERN.exec(body)?.[1].toLowerCase() ?? null;

export const trackingTaskIds = (gaps) => [
  ...new Set(gaps.map((gap) => trackingTaskId(gap.body)).filter(Boolean)),
];

const LIVE_TASK_STATUSES = new Set([
  "open",
  "claimed",
  "needs_input",
  "paused",
  "in_review",
  "blocked",
]);

/** Pick statuses for declared tracking ids from a bounded Lodestar board read. */
export const trackingStatusesFromBoard = (trackingIds, board) => {
  const tasks = Array.isArray(board) ? board : board?.tasks;
  if (!Array.isArray(tasks)) {
    throw new Error("Lodestar returned an unreadable task board");
  }
  const requested = new Set(trackingIds);
  return new Map(
    tasks
      .filter(
        (task) => requested.has(task.id) && typeof task.status === "string",
      )
      .map((task) => [task.id, task.status]),
  );
};

const trackingStatusesFromLodestar = (gaps) => {
  const ids = trackingTaskIds(gaps);
  if (ids.length === 0) return new Map();

  const repoRoot = execFileSync("git", ["rev-parse", "--show-toplevel"], {
    encoding: "utf8",
  }).trim();
  const server = resolveServer(repoRoot, "lodestar");
  if (!server) {
    throw new Error(
      "cannot verify declared tracking tasks: build lodestar-mcp or set LODESTAR_MCP_BIN",
    );
  }
  const [board] = callTools(
    server,
    repoRoot,
    [
      {
        name: "task_query",
        arguments: { view: "board", detail: false, limit: 0 },
      },
    ],
    8 * 1024 * 1024,
  );
  return trackingStatusesFromBoard(ids, board);
};

/**
 * Parse `git log --reverse --diff-filter=A --name-only --format=C:%ct`
 * output (oldest first) into `{ [path]: firstAddedUnixSeconds }`. Reverse
 * order plus "first path wins" means a later re-add of the same path (a
 * revert, a rename git didn't detect) never overwrites the fragment's true
 * original filing date.
 */
export const parseFirstAddedLog = (log) => {
  const firstSeen = {};
  let currentEpoch = null;
  for (const line of log.split("\n")) {
    if (line.startsWith("C:")) {
      currentEpoch = Number(line.slice(2));
      continue;
    }
    if (!line.trim() || currentEpoch === null) continue;
    if (!(line in firstSeen)) firstSeen[line] = currentEpoch;
  }
  return firstSeen;
};

export const ageDays = (nowMs, firstAddedSeconds) =>
  Math.floor((nowMs - firstAddedSeconds * 1000) / 86_400_000);

/**
 * The scorecard itself: age and task-linkage per fragment, plus the three
 * numbers that matter for "is this backlog actually moving" -- how many
 * fragments have no task at all (orphaned, i.e. nobody has committed to
 * fixing them), and the oldest/median age. A fragment `firstSeen` has no
 * entry for (uncommitted, or git history unavailable) reports age `null`
 * rather than being silently dropped from the count -- an unknown age is a
 * fact worth seeing, not a reason to hide the row.
 */
export const triageReport = (
  gaps,
  firstSeen,
  nowMs,
  taskStatuses = new Map(),
) => {
  const rows = gaps
    .map((gap) => {
      const seen = firstSeen[`${FRAGMENT_DIR}/${gap.name}`];
      const taskId = trackingTaskId(gap.body);
      const taskStatus = taskId === null ? null : taskStatuses.get(taskId);
      return {
        name: gap.name,
        ageDays: seen == null ? null : ageDays(nowMs, seen),
        taskId,
        taskStatus,
        hasLiveTracking:
          taskStatus !== undefined && LIVE_TASK_STATUSES.has(taskStatus),
      };
    })
    .sort((a, b) => (b.ageDays ?? -1) - (a.ageDays ?? -1));

  const knownAges = rows
    .map((row) => row.ageDays)
    .filter((age) => age != null)
    .sort((a, b) => a - b);
  const withLiveTracking = rows.filter((row) => row.hasLiveTracking).length;

  return {
    rows,
    total: rows.length,
    withLiveTracking,
    orphaned: rows.length - withLiveTracking,
    oldestAgeDays: rows[0]?.ageDays ?? null,
    medianAgeDays: knownAges.length
      ? knownAges[Math.floor(knownAges.length / 2)]
      : null,
  };
};

export const renderTriage = (report) => {
  const lines = [
    "gaps -- reliability scorecard (backlog age + task linkage)",
    "",
  ];
  for (const row of report.rows) {
    const age =
      row.ageDays == null ? "   ?" : `${String(row.ageDays).padStart(4)}d`;
    const tracking =
      row.taskId === null
        ? "none"
        : row.hasLiveTracking
          ? "live"
          : (row.taskStatus ?? "missing");
    lines.push(`  ${age}  task=${tracking.padEnd(7)}  ${row.name}`);
  }
  lines.push("");
  lines.push(`total: ${report.total} open fragment(s)`);
  lines.push(
    `tracked by a live task: ${report.withLiveTracking} (${report.orphaned} orphaned -- no live tracking task)`,
  );
  lines.push(
    `oldest: ${report.oldestAgeDays ?? "unknown"} day(s); median: ${
      report.medianAgeDays ?? "unknown"
    } day(s)`,
  );
  return lines.join("\n");
};

/**
 * The one impure step `--triage` needs: when each currently-tracked fragment
 * was first added. Isolated here so `triageReport`/`renderTriage` stay pure
 * and testable without a real git repository; returns `{}` (every age
 * unknown, never a thrown error) if git itself is unavailable.
 */
export const firstAddedDates = (dir = FRAGMENT_DIR) => {
  let log;
  try {
    log = execFileSync(
      "git",
      [
        "log",
        "--reverse",
        "--diff-filter=A",
        "--name-only",
        "--format=C:%ct",
        "--",
        `${dir}/*.md`,
      ],
      { encoding: "utf8" },
    );
  } catch {
    return {};
  }
  return parseFirstAddedLog(log);
};

const main = () => {
  const args = process.argv.slice(2);
  const { gaps, files, problems } = readFragments();

  if (problems.length) {
    console.error(`gaps: ${problems.length} unusable fragment(s)`);
    for (const problem of problems) console.error(`  ${problem}`);
    process.exit(1);
  }

  // An empty Known Gaps section is almost always a lie — DEVELOPERS.md says so
  // itself. A validator that passes over an empty directory would report success
  // for a repository that had quietly lost every gap it ever recorded, which is
  // the one result it must never give.
  if (gaps.length === 0) {
    console.error(
      `gaps: no fragments in ${FRAGMENT_DIR}/ — an empty Known Gaps section is almost\n` +
        `  always a lie, so this is treated as a missing directory rather than a clean bill\n` +
        `  of health. Record one, or say plainly in DEVELOPERS.md why there are none.`,
    );
    process.exit(1);
  }

  if (args.includes("--list")) {
    console.log(render(gaps));
    return;
  }

  if (args.includes("--check")) {
    console.log(`gaps: ${files.length} fragment(s) valid`);
    return;
  }

  if (args.includes("--triage")) {
    try {
      const taskStatuses = trackingStatusesFromLodestar(gaps);
      console.log(
        renderTriage(
          triageReport(gaps, firstAddedDates(), Date.now(), taskStatuses),
        ),
      );
    } catch (error) {
      console.error(`gaps: ${error.message}`);
      process.exitCode = 2;
    }
    return;
  }

  console.log(
    [
      "gaps -- known gaps are fragments, so recording one never conflicts",
      "",
      "  node scripts/gaps.mjs --check    validate fragments (hook/CI)",
      "  node scripts/gaps.mjs --list     print every open gap",
      "  node scripts/gaps.mjs --triage   reliability scorecard: backlog age + task linkage",
      "",
      `Add a gap: write ${FRAGMENT_DIR}/<slug>.md opening with a "- **" bullet.`,
      "Track a fix: add `Tracking: task:<12-hex>` on its own line; --triage verifies it is live.",
      "Close a gap: delete its fragment in the commit that fixes it.",
    ].join("\n"),
  );
};

if (
  process.argv[1] &&
  import.meta.url.endsWith(process.argv[1].replace(/\\/g, "/"))
) {
  main();
}
