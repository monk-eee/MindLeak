#!/usr/bin/env node
// PR-effectiveness telemetry: pure aggregation plus a thin read-only collector.

import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";
import { fileURLToPath } from "node:url";

import { resolveServer } from "./claim-gate.mjs";

const POLLING_TOOLS = new Set(["graph_stats", "telemetry_snapshot"]);

const ratio = (numerator, denominator) =>
  denominator > 0 ? numerator / denominator : null;

const values = (mapping, key) => {
  if (mapping instanceof Map) return mapping.get(key) ?? [];
  return mapping?.[key] ?? [];
};

export function parseEvidence(audit) {
  const raw = audit?.evidence;
  if (raw && typeof raw === "object") return raw;
  if (typeof raw !== "string" || raw.length === 0) return {};
  try {
    return JSON.parse(raw);
  } catch {
    return {};
  }
}

export function evidenceCommitIds(audits = []) {
  const ids = new Set();
  for (const audit of audits) {
    for (const id of parseEvidence(audit).commit_ids ?? []) {
      ids.add(String(id).replace(/^intent:/, ""));
    }
  }
  return ids;
}

export function extractPrNumbers(thread = []) {
  const numbers = new Set();
  const pattern = /(?:github\.com\/[^/\s]+\/[^/\s]+\/pull\/|\bPR\s*#)(\d+)/gi;
  for (const entry of thread) {
    const body = String(entry?.body ?? "");
    for (const match of body.matchAll(pattern)) numbers.add(Number(match[1]));
  }
  return numbers;
}

export function requiredCheckSummary(checks = [], required = []) {
  const byName = new Map(checks.map((check) => [check.name, check]));
  const requiredPresent =
    required.length > 0 && required.every((name) => byName.has(name));
  const pending = required.filter((name) => {
    const check = byName.get(name);
    return check && check.status !== "COMPLETED";
  });
  const failing = required.filter((name) => {
    const check = byName.get(name);
    return check?.status === "COMPLETED" && check.conclusion !== "SUCCESS";
  });
  return {
    required: [...required],
    required_present: requiredPresent,
    pending,
    failing,
    final_head_green:
      requiredPresent && pending.length === 0 && failing.length === 0,
  };
}

export function linkPrToTasks(pr, tasks, threadsByTask, auditsByTask) {
  const prCommits = new Set(
    [
      ...(pr.commits ?? []).map((commit) => commit.oid),
      pr.headRefOid,
      pr.mergeCommit?.oid,
    ].filter(Boolean),
  );
  const links = [];

  for (const task of tasks) {
    const methods = [];
    if (task.branch && task.branch === pr.headRefName) methods.push("branch");
    if (extractPrNumbers(values(threadsByTask, task.id)).has(pr.number)) {
      methods.push("task_thread");
    }
    const evidenceCommits = evidenceCommitIds(values(auditsByTask, task.id));
    if ([...evidenceCommits].some((commit) => prCommits.has(commit))) {
      methods.push("evidence_commit");
    }
    if (methods.length > 0) links.push({ task_id: task.id, methods });
  }
  return links;
}

export function receiptCategory(audits = []) {
  if (audits.length === 0) return "no_conformance";
  const latest = [...audits].sort(
    (a, b) => (b.checked_at ?? 0) - (a.checked_at ?? 0),
  )[0];
  const evidence = parseEvidence(latest);
  const mutations = [
    ...(evidence.changed_node_ids ?? []),
    ...(evidence.commit_ids ?? []),
    ...(evidence.successful_execution_ids ?? []),
  ];
  if (mutations.length === 0) return "empty_evidence";
  return String(latest.verdict ?? "unknown").toLowerCase();
}

export function findingCauses(audits = []) {
  const causes = new Set();
  for (const audit of audits) {
    const findings = String(audit?.findings ?? "").toLowerCase();
    if (findings.includes("no provenance-bearing mutation"))
      causes.add("empty_evidence");
    if (
      findings.includes("lease lapsed") ||
      findings.includes("window is discontinuous")
    ) {
      causes.add("lease_discontinuity");
    }
    if (
      findings.includes("without a covering task") ||
      findings.includes("does not touch code")
    ) {
      causes.add("goal_coverage");
    }
    if (findings.includes("required check")) causes.add("required_checks");
  }
  return [...causes].sort();
}

const firstCommitAt = (pr) => {
  const times = (pr.commits ?? [])
    .map((commit) => Date.parse(commit.committedDate))
    .filter(Number.isFinite)
    .sort((a, b) => a - b);
  return times.length > 0 ? Math.floor(times[0] / 1000) : null;
};

const reconciliationCount = (pr) =>
  (pr.commits ?? []).filter((commit) =>
    /^Merge (?:branch|remote-tracking branch) .*main/i.test(
      commit.messageHeadline ?? "",
    ),
  ).length;

const claimStartedAt = (task) =>
  typeof task?.claim_started_at === "number"
    ? task.claim_started_at
    : typeof task?.claim_window?.started_at === "number"
      ? task.claim_window.started_at
      : null;

export function summarizeRuntime(snapshot = {}) {
  const metrics = snapshot.by_name ?? [];
  const calls = metrics.reduce(
    (sum, metric) => sum + Number(metric.calls ?? 0),
    0,
  );
  const pollingCalls = metrics
    .filter((metric) => POLLING_TOOLS.has(metric.name))
    .reduce((sum, metric) => sum + Number(metric.calls ?? 0), 0);
  const latencies = (snapshot.recent ?? [])
    .map((event) => event.duration_ms)
    .filter((duration) => Number.isFinite(duration));
  const recentErrors = (snapshot.recent ?? [])
    .filter((event) => event.outcome === "error")
    .map(({ ts, name, detail }) => ({
      ts,
      name,
      category: detail?.category ?? null,
    }));
  const habits = snapshot.memory_habits ?? [];
  const decidedHabits = habits.filter(
    (habit) => habit.read_before_first_write !== null,
  );
  const adopted = decidedHabits.filter(
    (habit) => habit.read_before_first_write === true,
  ).length;

  return {
    total_events: Number(snapshot.total_events ?? 0),
    total_errors: Number(snapshot.total_errors ?? 0),
    currently_failing_tools: Number(snapshot.currently_failing_tools ?? 0),
    lifetime_error_rate: ratio(
      snapshot.total_errors ?? 0,
      snapshot.total_events ?? 0,
    ),
    polling_calls: pollingCalls,
    polling_share: ratio(pollingCalls, calls),
    recent_latency_ms: {
      samples: latencies.length,
      average: latencies.length
        ? latencies.reduce((sum, value) => sum + value, 0) / latencies.length
        : null,
      maximum: latencies.length ? Math.max(...latencies) : null,
    },
    recent_errors: recentErrors,
    memory_habits: habits,
    memory_read_before_write: {
      eligible_sessions: decidedHabits.length,
      adopted_sessions: adopted,
      adoption_rate: ratio(adopted, decidedHabits.length),
    },
  };
}

export function analyzeProduction({
  prs = [],
  tasks = [],
  threadsByTask = {},
  auditsByTask = {},
  requiredChecks = [],
}) {
  const taskById = new Map(tasks.map((task) => [task.id, task]));
  const linkedTaskIds = new Set();
  const categories = {};
  let linkedTasksWithConformance = 0;
  const summarizedTaskIds = new Set();
  const earliestCommitByTask = new Map();
  let humanEligible = 0;
  let humanResolved = 0;
  let reconciliationMerges = 0;

  const rows = prs.map((pr) => {
    const links = linkPrToTasks(pr, tasks, threadsByTask, auditsByTask);
    const commitAt = firstCommitAt(pr);
    const linkedTasks = links.map((link) => {
      const task = taskById.get(link.task_id);
      const audits = values(auditsByTask, link.task_id);
      const category = receiptCategory(audits);
      linkedTaskIds.add(link.task_id);
      if (commitAt !== null) {
        const prior = earliestCommitByTask.get(link.task_id);
        earliestCommitByTask.set(
          link.task_id,
          prior === undefined ? commitAt : Math.min(prior, commitAt),
        );
      }
      let claimedBeforeFirstCommit = null;
      const claimedAt = claimStartedAt(task);
      if (commitAt !== null && claimedAt !== null) {
        claimedBeforeFirstCommit = claimedAt <= commitAt;
      }
      if (!summarizedTaskIds.has(link.task_id)) {
        summarizedTaskIds.add(link.task_id);
        if (audits.length > 0) {
          linkedTasksWithConformance += 1;
          categories[category] = (categories[category] ?? 0) + 1;
        }
        if (["drift", "needs_human", "violation"].includes(category)) {
          humanEligible += 1;
          if (task?.resolved_by) humanResolved += 1;
        }
      }
      return {
        ...link,
        status: task?.status ?? null,
        receipt: category,
        finding_causes: findingCauses(audits),
        resolved_by: task?.resolved_by ?? null,
        claimed_before_first_commit: claimedBeforeFirstCommit,
      };
    });
    const churn = reconciliationCount(pr);
    reconciliationMerges += churn;
    return {
      number: pr.number,
      branch: pr.headRefName,
      state: pr.state,
      merged_at: pr.mergedAt ?? null,
      links: linkedTasks,
      attribution_complete: linkedTasks.length > 0,
      checks: requiredCheckSummary(pr.statusCheckRollup ?? [], requiredChecks),
      reconciliation_merges: churn,
    };
  });

  const merged = prs.filter((pr) => pr.state === "MERGED").length;
  const open = prs.filter((pr) => pr.state === "OPEN").length;
  const closedWithoutMerge = prs.filter((pr) => pr.state === "CLOSED").length;
  let claimBefore = 0;
  let claimAfter = 0;
  let claimUnknown = 0;
  for (const taskId of linkedTaskIds) {
    const claimedAt = claimStartedAt(taskById.get(taskId));
    const commitAt = earliestCommitByTask.get(taskId);
    if (claimedAt === null || commitAt === undefined) claimUnknown += 1;
    else if (claimedAt <= commitAt) claimBefore += 1;
    else claimAfter += 1;
  }
  return {
    all_available_prs: {
      prs: prs.length,
      merged,
      open,
      closed_without_merge: closedWithoutMerge,
      linked_prs: rows.filter((row) => row.attribution_complete).length,
      unlinked_prs: rows.filter((row) => !row.attribution_complete).length,
      linked_tasks: linkedTaskIds.size,
      linked_tasks_with_conformance: linkedTasksWithConformance,
      linked_tasks_without_conformance:
        linkedTaskIds.size - linkedTasksWithConformance,
      receipt_categories: categories,
      claim_before_first_commit: {
        before: claimBefore,
        after: claimAfter,
        unknown: claimUnknown,
        rate: ratio(claimBefore, claimBefore + claimAfter),
      },
      human_resolution: {
        eligible: humanEligible,
        resolved: humanResolved,
        rate: ratio(humanResolved, humanEligible),
      },
      reconciliation_merges: reconciliationMerges,
    },
    pull_requests: rows,
  };
}

export function controlledSyntheticBenchmark() {
  const required = ["build", "test"];
  const tasks = [
    { id: "task:branch", branch: "fix/branch", claim_started_at: 100 },
    { id: "task:thread", claim_started_at: 100 },
    { id: "task:evidence", claim_started_at: 300 },
  ];
  const audits = {
    "task:branch": [
      {
        verdict: "aligned",
        checked_at: 200,
        evidence: { changed_node_ids: ["a"] },
      },
    ],
    "task:evidence": [
      {
        verdict: "needs_human",
        checked_at: 400,
        evidence: { commit_ids: ["intent:abc"] },
      },
    ],
  };
  const threads = { "task:thread": [{ body: "Delivered in PR #7" }] };
  const pr = {
    number: 7,
    headRefName: "fix/branch",
    headRefOid: "abc",
    state: "MERGED",
    mergedAt: "2026-01-01T00:00:00Z",
    commits: [
      {
        oid: "abc",
        committedDate: "1970-01-01T00:03:20Z",
        messageHeadline: "change",
      },
    ],
    statusCheckRollup: [
      { name: "build", status: "COMPLETED", conclusion: "SUCCESS" },
    ],
  };
  const production = analyzeProduction({
    prs: [pr],
    tasks,
    threadsByTask: threads,
    auditsByTask: audits,
    requiredChecks: required,
  });
  const links = production.pull_requests[0].links;
  const gates = [
    {
      name: "all explicit linkage methods are observable",
      pass: ["branch", "task_thread", "evidence_commit"].every((method) =>
        links.some((link) => link.methods.includes(method)),
      ),
    },
    {
      name: "missing required checks are never green",
      pass: production.pull_requests[0].checks.final_head_green === false,
    },
    {
      name: "incomplete attribution remains visible",
      pass:
        analyzeProduction({
          prs: [{ ...pr, number: 8, headRefName: "none", headRefOid: "none" }],
          tasks,
        }).pull_requests[0].attribution_complete === false,
    },
  ];
  return {
    purpose:
      "Deterministic correctness controls for linkage and missing data; not production efficacy.",
    gates,
    passed: gates.filter((gate) => gate.pass).length,
    total: gates.length,
  };
}

export function renderMarkdown(report) {
  const p = report.production.all_available_prs;
  const r = report.runtime;
  const pct = (value) =>
    value === null ? "unknown" : `${(value * 100).toFixed(1)}%`;
  return [
    "# PR effectiveness telemetry",
    "",
    `Generated: ${report.generated_at}`,
    "",
    "## Evidence tiers",
    "",
    `- Runtime health: ${r.total_events} events; lifetime error rate ${pct(r.lifetime_error_rate)}; polling share ${pct(r.polling_share)}.`,
    `- Production correlation: ${p.linked_prs}/${p.prs} PRs linked to ${p.linked_tasks} task(s).`,
    `- Controlled synthetic: ${report.controlled_synthetic.passed}/${report.controlled_synthetic.total} correctness gates passed.`,
    "",
    "## Production",
    "",
    `- PR states: ${p.merged} merged, ${p.open} open, ${p.closed_without_merge} closed without merge.`,
    `- Claim before first commit: ${pct(p.claim_before_first_commit.rate)} (${p.claim_before_first_commit.unknown} unknown).`,
    `- Human resolution rate: ${pct(p.human_resolution.rate)}.`,
    `- Reconciliation merge commits: ${p.reconciliation_merges}.`,
    `- Memory read before first write: ${pct(r.memory_read_before_write.adoption_rate)}.`,
    "",
    "## Interpretation limits",
    "",
    ...report.limitations.map((limitation) => `- ${limitation}`),
    "",
  ].join("\n");
}

const runJson = (command, args, options = {}) => {
  const result = spawnSync(command, args, {
    cwd: options.cwd,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error || result.status !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} failed: ${result.error?.message ?? result.stderr ?? result.stdout}`,
    );
  }
  return JSON.parse(result.stdout);
};

const git = (cwd, args) => {
  const result = spawnSync("git", args, { cwd, encoding: "utf8" });
  if (result.error || result.status !== 0) {
    throw new Error(
      `git ${args.join(" ")} failed: ${result.error?.message ?? result.stderr}`,
    );
  }
  return result.stdout.trim();
};

const serverFor = (root, plane) => {
  const override =
    process.env[plane === "lodestar" ? "LODESTAR_MCP_BIN" : "MINDLEAK_MCP_BIN"];
  if (override && fs.existsSync(override)) return override;
  const exe = `${plane}-mcp${process.platform === "win32" ? ".exe" : ""}`;
  const stable = path.join(os.homedir(), ".mindleak", "bin", exe);
  if (fs.existsSync(stable)) return stable;
  return resolveServer(root, plane);
};

const startMcp = async (binary, cwd, clientName) => {
  if (!binary) throw new Error(`no ${clientName} binary found`);
  const [command, leading] = binary.endsWith(".mjs")
    ? [process.execPath, [binary]]
    : [binary, []];
  const child = spawn(command, leading, {
    cwd,
    stdio: ["pipe", "pipe", "pipe"],
  });
  const pending = new Map();
  let nextId = 1;
  let stderr = "";
  child.stderr.on("data", (chunk) => (stderr += chunk.toString()));
  readline.createInterface({ input: child.stdout }).on("line", (line) => {
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      return;
    }
    const waiter = pending.get(message.id);
    if (!waiter) return;
    pending.delete(message.id);
    clearTimeout(waiter.timer);
    if (message.error) waiter.reject(new Error(message.error.message));
    else waiter.resolve(message.result);
  });
  const request = (method, params) =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(
          new Error(`${clientName} ${method} timed out; ${stderr.trim()}`),
        );
      }, 60_000);
      pending.set(id, { resolve, reject, timer });
      child.stdin.write(
        `${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`,
      );
    });
  const call = async (name, args = {}) => {
    const result = await request("tools/call", { name, arguments: args });
    if (result?.isError)
      throw new Error(result.content?.[0]?.text ?? `${name} failed`);
    if (result?.structuredContent !== undefined)
      return result.structuredContent;
    const text = result?.content?.[0]?.text;
    if (text === undefined) return null;
    try {
      return JSON.parse(text);
    } catch {
      return text;
    }
  };
  await request("initialize", {
    protocolVersion: "2024-11-05",
    capabilities: {},
    clientInfo: { name: clientName, version: "1" },
  });
  return { call, close: () => child.kill() };
};

const mapLimit = async (items, limit, visit) => {
  const results = new Array(items.length);
  let next = 0;
  const worker = async () => {
    while (next < items.length) {
      const index = next++;
      results[index] = await visit(items[index], index);
    }
  };
  await Promise.all(
    Array.from({ length: Math.min(limit, items.length) }, worker),
  );
  return results;
};

const parseOptions = (args) => {
  const limitArg = args.find((arg) => arg.startsWith("--limit="));
  const outputArg = args.find((arg) => arg.startsWith("--output-dir="));
  const limit = Number(limitArg?.split("=")[1] ?? 50);
  if (!Number.isInteger(limit) || limit < 1 || limit > 100) {
    throw new Error("--limit must be an integer from 1 to 100");
  }
  return {
    limit,
    outputDir: outputArg?.slice("--output-dir=".length) ?? "target/telemetry",
  };
};

const collect = async ({ root, limit }) => {
  const warnings = [];
  const repo = runJson(
    "gh",
    ["repo", "view", "--json", "nameWithOwner,defaultBranchRef"],
    {
      cwd: root,
    },
  );
  const ownerRepo = repo.nameWithOwner;
  const base = repo.defaultBranchRef?.name ?? "main";
  const required =
    runJson("gh", [
      "api",
      `repos/${ownerRepo}/branches/${base}/protection/required_status_checks`,
    ]).contexts ?? [];
  const summaries = runJson("gh", [
    "pr",
    "list",
    "--repo",
    ownerRepo,
    "--base",
    base,
    "--state",
    "all",
    "--limit",
    String(limit),
    "--json",
    "number,headRefName,state,isDraft,createdAt,updatedAt,mergedAt,closedAt",
  ]);
  const prs = summaries.map((summary) => {
    try {
      const detail = runJson("gh", [
        "pr",
        "view",
        String(summary.number),
        "--repo",
        ownerRepo,
        "--json",
        "headRefOid,mergeCommit,statusCheckRollup,commits",
      ]);
      return { ...summary, ...detail };
    } catch (error) {
      warnings.push(`GitHub PR #${summary.number}: ${error.message}`);
      return {
        ...summary,
        headRefOid: null,
        mergeCommit: null,
        statusCheckRollup: [],
        commits: [],
      };
    }
  });

  const lodestar = await startMcp(
    serverFor(root, "lodestar"),
    root,
    "pr-effectiveness-lodestar",
  );
  const mindleak = await startMcp(
    serverFor(root, "mindleak"),
    root,
    "pr-effectiveness-mindleak",
  );
  try {
    const board = await lodestar.call("task_query", {
      view: "board",
      include_terminal: true,
      // claimStartedAt() below only ever reads the base claim_started_at
      // field; scope/claim_window/receipt/acceptance are never used.
      detail: false,
    });
    const tasks = Array.isArray(board) ? board : Object.values(board ?? {});
    const threadsByTask = {};
    const auditsByTask = {};
    await mapLimit(tasks, 8, async (task) => {
      try {
        threadsByTask[task.id] = await lodestar.call("task_query", {
          view: "thread",
          task_id: task.id,
        });
      } catch (error) {
        warnings.push(`thread ${task.id}: ${error.message}`);
        threadsByTask[task.id] = [];
      }
      try {
        auditsByTask[task.id] = await lodestar.call("conformance_history", {
          task_id: task.id,
        });
      } catch (error) {
        warnings.push(`conformance ${task.id}: ${error.message}`);
        auditsByTask[task.id] = [];
      }
    });
    const telemetry = await mindleak.call("telemetry_snapshot", { limit: 500 });
    return {
      ownerRepo,
      base,
      required,
      prs,
      tasks,
      threadsByTask,
      auditsByTask,
      telemetry,
      warnings,
    };
  } finally {
    lodestar.close();
    mindleak.close();
  }
};

const validateReport = (report) => {
  const failures = [];
  const production = report.production.all_available_prs;
  if (
    production.merged + production.open + production.closed_without_merge !==
    production.prs
  ) {
    failures.push("PR state counts do not sum to the cohort");
  }
  if (
    report.production.pull_requests.some(
      (row) => row.checks.final_head_green && !row.checks.required_present,
    )
  ) {
    failures.push("a PR is green without every required check present");
  }
  const categories = Object.values(production.receipt_categories).reduce(
    (sum, count) => sum + count,
    0,
  );
  if (categories !== production.linked_tasks_with_conformance) {
    failures.push(
      "receipt categories do not sum to linked tasks with conformance",
    );
  }
  if (
    report.controlled_synthetic.passed !== report.controlled_synthetic.total
  ) {
    failures.push("controlled synthetic gates failed");
  }
  return failures;
};

async function main() {
  const root = git(process.cwd(), ["rev-parse", "--show-toplevel"]);
  const options = parseOptions(process.argv.slice(2));
  const generatedAt = new Date().toISOString();
  const collected = await collect({ root, limit: options.limit });
  const production = analyzeProduction({
    prs: collected.prs,
    tasks: collected.tasks,
    threadsByTask: collected.threadsByTask,
    auditsByTask: collected.auditsByTask,
    requiredChecks: collected.required,
  });
  const report = {
    schema_version: 1,
    generated_at: generatedAt,
    source_revision: git(root, ["rev-parse", "HEAD"]),
    source_dirty: git(root, ["status", "--porcelain"]).length > 0,
    cohort: {
      repository: collected.ownerRepo,
      base: collected.base,
      limit: options.limit,
      returned: collected.prs.length,
      pr_numbers: collected.prs.map((pr) => pr.number),
    },
    data_sources: {
      github_prs: collected.prs.length,
      lodestar_tasks: collected.tasks.length,
      mindleak_recent_events: collected.telemetry.recent?.length ?? 0,
    },
    runtime: summarizeRuntime(collected.telemetry),
    production,
    controlled_synthetic: controlledSyntheticBenchmark(),
    limitations: [
      "MindLeak tool events do not carry PR ids; production links use explicit task branch, task-thread PR reference, or evidence commit provenance.",
      "The bounded GitHub cohort is observational and is not a randomized efficacy experiment.",
      "Calling telemetry_snapshot appends the server's normal tool-call telemetry after the snapshot; reported values describe the instant before that observer event.",
      "Missing required check data is unknown, never green.",
    ],
    collection_warnings: collected.warnings,
  };
  report.validation_failures = validateReport(report);

  const outputDir = path.resolve(root, options.outputDir);
  fs.mkdirSync(outputDir, { recursive: true });
  const stamp = generatedAt.replace(/[:.]/g, "-");
  const baseName = `${stamp}-pr-effectiveness`;
  const jsonPath = path.join(outputDir, `${baseName}.json`);
  const markdownPath = path.join(outputDir, `${baseName}.md`);
  fs.writeFileSync(jsonPath, `${JSON.stringify(report, null, 2)}\n`);
  fs.writeFileSync(markdownPath, renderMarkdown(report));
  console.log(`pr-effectiveness: ${jsonPath}`);
  console.log(`pr-effectiveness: ${markdownPath}`);
  console.log(
    `pr-effectiveness: ${production.all_available_prs.linked_prs}/${production.all_available_prs.prs} PRs linked; ${report.validation_failures.length} validation failure(s); ${report.collection_warnings.length} collection warning(s)`,
  );
  if (
    report.validation_failures.length > 0 ||
    report.collection_warnings.length > 0
  ) {
    process.exitCode = 1;
  }
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  main().catch((error) => {
    console.error(`pr-effectiveness: ${error.stack ?? error.message}`);
    process.exitCode = 1;
  });
}
