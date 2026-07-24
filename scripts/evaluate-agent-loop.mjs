// Real agent-in-the-loop outcome benchmark using GitHub Copilot CLI.
//
// Four randomized arms run the same pinned model in fresh fixture directories:
//   none                no durable context
//   flat                query-blind recent logs in the prompt
//   mindleak            seeded MindLeak MCP tools
//   mindleak+lodestar   seeded MindLeak + governing Lodestar tools
//
// The task combines interrupted-work recovery, a cross-file regression,
// impacted-file prediction, failed-approach avoidance, and an invariant. Hidden
// checks score correctness; JSONL tool events score exploration and cost.

import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";
import { spawn, spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";

const root = process.cwd();
const model = "claude-haiku-4.5";
const copilotVersion = spawnSync("copilot", ["--version"], {
  encoding: "utf8",
})
  .stdout.split(/\r?\n/)[0]
  .trim()
  .replace(/\.$/, "");
const revision = spawnSync("git", ["rev-parse", "--short", "HEAD"], {
  cwd: root,
  encoding: "utf8",
}).stdout.trim();
const dirty =
  spawnSync("git", ["status", "--porcelain"], {
    cwd: root,
    encoding: "utf8",
  }).stdout.trim().length > 0;
const repeatsArg = process.argv.find((argument) =>
  argument.startsWith("--repeats="),
);
const repeats = Number(repeatsArg?.split("=")[1] ?? 3);
if (!Number.isInteger(repeats) || repeats < 1 || repeats > 10) {
  throw new Error("--repeats must be an integer from 1 to 10");
}
const mindleakExe = path.join(
  root,
  "target",
  "debug",
  process.platform === "win32" ? "mindleak-mcp.exe" : "mindleak-mcp",
);
const lodestarExe = path.join(
  root,
  "target",
  "debug",
  process.platform === "win32" ? "lodestar-mcp.exe" : "lodestar-mcp",
);
for (const executable of [mindleakExe, lodestarExe]) {
  if (!fs.existsSync(executable)) {
    throw new Error(`missing ${executable}; build both MCP servers first`);
  }
}

const fixture = {
  "package.json": `${JSON.stringify({ type: "module", scripts: { test: "node public-test.mjs" } }, null, 2)}\n`,
  "src/auth.js": `export function validateSession(ticket, now = Date.now()) {
  return typeof ticket === "string" && ticket.length > 0;
}
`,
  "src/login.js": `import { validateSession } from "./auth.js";

export function handleLogin(request, now = Date.now()) {
  return validateSession(request.ticket, now);
}
`,
  "src/middleware.js": `import { validateSession } from "./auth.js";

export function requireAuth(request, response, next, now = Date.now()) {
  if (!validateSession(request.ticket, now)) {
    response.status(401);
    return false;
  }
  next();
  return true;
}
`,
  "src/auth-legacy.js": `export function validateLegacySession(ticket) {
  return Boolean(ticket);
}
`,
  "src/token-utils.js": `export function stringifyTicket(ticket) {
  return String(ticket?.value ?? ticket ?? "");
}
`,
  "public-test.mjs": `import assert from "node:assert/strict";
import { handleLogin } from "./src/login.js";
import { requireAuth } from "./src/middleware.js";

const now = 1_000;
const ticket = { value: "session-123", expiresAt: now + 60 };
assert.equal(handleLogin({ ticket }, now), true);
let nextCalled = false;
const response = { statusCode: 200, status(code) { this.statusCode = code; } };
assert.equal(requireAuth({ ticket }, response, () => { nextCalled = true; }, now), true);
assert.equal(nextCalled, true);
console.log("public tests passed");
`,
};
const protectedHashes = new Map(
  ["package.json", "public-test.mjs"].map((file) => [
    file,
    hash(fixture[file]),
  ]),
);
const impactTruth = new Set([
  "src/auth.js",
  "src/login.js",
  "src/middleware.js",
]);
const flatContext = `Recent flat execution/commit history (query-blind, newest first):
- green: formatting checks passed
- green: documentation checks passed
- green: package audit passed
- green: unrelated math tests passed
- failure: node public-test.mjs rejected a valid session ticket in src/login.js
`;
const basePrompt = `Fix the session validation regression in this fixture. A valid typed session ticket is rejected and the expiry boundary is wrong. Preserve prior architectural intent. Do not access the network or files outside this workspace. Run node public-test.mjs. Do not modify package.json or public-test.mjs.

Create RESULT.json with exactly this shape:
{"impacted_files":["workspace-relative production files affected by the contract"],"summary":"short explanation"}

"impacted_files" means every production file whose behavior depends on the validateSession contract, including unchanged callers; it is not merely the list of files you edited. Exclude tests and RESULT.json.
`;
const arms = ["none", "flat", "mindleak", "mindleak+lodestar"];
const outputRoot = path.join(root, "target", "agent-outcomes");
fs.rmSync(outputRoot, { recursive: true, force: true });
fs.mkdirSync(outputRoot, { recursive: true });
const workspaceRoot = path.join(outputRoot, "workspaces");
const homeRoot = path.join(outputRoot, "homes");
fs.mkdirSync(workspaceRoot, { recursive: true });
fs.mkdirSync(homeRoot, { recursive: true });

const schedule = [];
for (let repeat = 0; repeat < repeats; repeat += 1) {
  for (const arm of shuffled(arms, repeat + 73)) {
    schedule.push({ repeat, arm });
  }
}

const runs = [];
for (const item of schedule) {
  const runName = `r${item.repeat + 1}-${item.arm.replaceAll("+", "-")}`;
  const directory = path.join(workspaceRoot, runName);
  const copilotHome = path.join(homeRoot, runName);
  fs.mkdirSync(directory, { recursive: true });
  prepareCopilotHome(copilotHome);
  for (const [file, content] of Object.entries(fixture)) {
    const target = path.join(directory, file);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, content);
  }

  let context = "";
  let mcpConfig;
  const sessionId = crypto.randomBytes(16).toString("hex");
  if (item.arm === "flat") {
    context = flatContext;
  }
  if (item.arm.includes("mindleak")) {
    const graphDb = path.join(directory, "graph.db");
    await seedMindLeak(directory, graphDb, sessionId);
    mcpConfig = {
      mcpServers: {
        "mindleak-eval": {
          command: mindleakExe,
          args: [],
          env: {
            MINDLEAK_DB: graphDb,
            MINDLEAK_AGENT: "eval-agent",
            MINDLEAK_LOG: "off",
          },
        },
      },
    };
    context =
      "Before reading source: call MindLeak get_impact_radius for artifact:src/auth.js, then graph_multi_hop_query for `typed session ticket string conversion failure expiresAt` with max_depth 2. Treat those results as durable evidence and inspect only files needed to edit or verify. Use impacted artifacts in RESULT.json.\n";
  }
  if (item.arm === "mindleak+lodestar") {
    const lodestarDb = path.join(directory, "lodestar.db");
    await seedLodestar(directory, lodestarDb, sessionId);
    mcpConfig.mcpServers["lodestar-eval"] = {
      command: lodestarExe,
      args: [],
      env: {
        LODESTAR_DB: lodestarDb,
        LODESTAR_AGENT: "eval-agent",
        LODESTAR_LLM_URL: "http://127.0.0.1:1/v1",
      },
    };
    context =
      "Before reading source: call Lodestar get_constitution and board; call MindLeak get_impact_radius for artifact:src/auth.js and graph_multi_hop_query for `typed session ticket string conversion failure expiresAt` with max_depth 2. Treat returned intent and evidence as authoritative and inspect only files needed to edit or verify. Use impacted artifacts in RESULT.json.\n";
  }
  if (mcpConfig) {
    fs.writeFileSync(
      path.join(directory, "mcp.json"),
      `${JSON.stringify(mcpConfig, null, 2)}\n`,
    );
  }

  const prompt = `${context}${basePrompt}`;
  const args = [
    "-C",
    directory,
    "-p",
    prompt,
    "--model",
    model,
    "--output-format",
    "json",
    "--stream",
    "off",
    "--no-custom-instructions",
    "--disable-builtin-mcps",
    "--no-auto-update",
    "--no-remote",
    "--no-ask-user",
    "--allow-all-tools",
    "--deny-url",
    "--disallow-temp-dir",
    "--log-level",
    "none",
  ];
  if (mcpConfig) {
    args.push("--additional-mcp-config", "@mcp.json");
  }

  const started = Date.now();
  const processResult = spawnSync("copilot", args, {
    cwd: root,
    env: { ...process.env, COPILOT_HOME: copilotHome },
    encoding: "utf8",
    timeout: 10 * 60 * 1000,
    maxBuffer: 64 * 1024 * 1024,
  });
  const elapsedMs = Date.now() - started;
  const stdout = processResult.stdout ?? "";
  const stderr = processResult.stderr ?? "";
  fs.writeFileSync(path.join(directory, "agent.jsonl"), stdout);
  fs.writeFileSync(path.join(directory, "agent.stderr.txt"), stderr);
  const events = parseJsonLines(stdout);
  const score = await evaluateWorkspace(directory);
  const metrics = summarizeEvents(events);
  runs.push({
    arm: item.arm,
    repeat: item.repeat + 1,
    exit_code: processResult.status,
    timed_out:
      processResult.signal === "SIGTERM" ||
      Boolean(processResult.error?.message.includes("ETIMEDOUT")),
    elapsed_ms: elapsedMs,
    ...metrics,
    ...score,
  });
  console.log(
    `${item.arm} r${item.repeat + 1}: success=${score.task_success} exploration=${metrics.exploration_tool_calls} tools=${metrics.total_tool_calls} hidden=${score.hidden_passed}/${score.hidden_total}`,
  );
}

const summary = Object.fromEntries(
  arms.map((arm) => {
    const armRuns = runs.filter((run) => run.arm === arm);
    const successes = armRuns.filter((run) => run.task_success).length;
    const exploration = armRuns
      .map((run) => run.exploration_tool_calls)
      .sort((a, b) => a - b);
    const durations = armRuns
      .map((run) => run.elapsed_ms)
      .sort((a, b) => a - b);
    const tokens = armRuns
      .map((run) => run.output_tokens)
      .sort((a, b) => a - b);
    return [
      arm,
      {
        runs: armRuns.length,
        task_success_rate: successes / armRuns.length,
        regression_rate:
          armRuns.filter((run) => run.regression_count > 0).length /
          armRuns.length,
        impacted_file_mean_f1:
          armRuns.reduce((sum, run) => sum + run.impacted_files.f1, 0) /
          armRuns.length,
        exploration_tool_calls_median: median(exploration),
        exploration_tool_calls_variance: variance(exploration),
        output_tokens_median: median(tokens),
        duration_ms_median: median(durations),
      },
    ];
  }),
);
const control = summary.none;
const memoryCandidates = [summary.mindleak, summary["mindleak+lodestar"]];
const bestExploration = Math.min(
  ...memoryCandidates.map((entry) => entry.exploration_tool_calls_median),
);
const bestSuccess = Math.max(
  ...memoryCandidates.map((entry) => entry.task_success_rate),
);
const explorationReduction =
  control.exploration_tool_calls_median === 0
    ? 0
    : (control.exploration_tool_calls_median - bestExploration) /
      control.exploration_tool_calls_median;
const successImprovement = bestSuccess - control.task_success_rate;
const noCorrectnessRegression = memoryCandidates.every(
  (entry) => entry.regression_rate <= control.regression_rate,
);
const gate = {
  exploration_reduction: explorationReduction,
  success_rate_improvement: successImprovement,
  no_correctness_regression: noCorrectnessRegression,
  passed:
    noCorrectnessRegression &&
    (explorationReduction >= 0.15 || successImprovement >= 0.1),
};
const report = {
  schema_version: 1,
  captured_at: new Date().toISOString(),
  source_revision: dirty ? `${revision}-dirty` : revision,
  runner: `${copilotVersion} / ${model}`,
  mindleak_executable_sha256: hash(fs.readFileSync(mindleakExe)),
  lodestar_executable_sha256: hash(fs.readFileSync(lodestarExe)),
  fixture_sha256: hash(
    Object.entries(fixture)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([file, content]) => `${file}\0${content}`)
      .join("\0"),
  ),
  isolation: {
    fresh_workspace_per_run: true,
    fresh_databases_per_run: true,
    isolated_copilot_home_per_run: true,
    personal_skills_mcp_memory_sessions_loaded: false,
    builtin_github_mcp_disabled: true,
    network_tools_denied: true,
  },
  repeats,
  scenario: "resume typed-session regression with impact and invariant",
  schedule,
  summary,
  gate,
  runs,
};
const resultPath = path.join(
  root,
  "benchmarks",
  "results",
  "2026-07-22-agent-loop-outcome.json",
);
fs.writeFileSync(resultPath, `${JSON.stringify(report, null, 2)}\n`);
console.log(JSON.stringify({ summary, gate }, null, 2));
console.log(`Wrote ${path.relative(root, resultPath)}`);
if (!gate.passed) {
  process.exitCode = 1;
}

async function seedMindLeak(directory, database, sessionId) {
  const server = await startMcp(
    mindleakExe,
    directory,
    {
      MINDLEAK_DB: database,
      MINDLEAK_AGENT: "eval-agent",
      MINDLEAK_LOG: "off",
    },
    sessionId,
  );
  try {
    for (const file of [
      "src/auth.js",
      "src/login.js",
      "src/middleware.js",
      "src/auth-legacy.js",
      "src/token-utils.js",
    ]) {
      await server.tool("ingest_file", { path: file, content: fixture[file] });
    }
    await server.tool("ingest_execution", {
      command: "node public-test.mjs",
      exit_code: 1,
      output:
        "AssertionError: valid typed ticket rejected\n    at handleLogin (src/login.js:4:10)\n    at public-test.mjs:7:8",
      changed_files: [],
      timestamp: 1_999_999_900,
    });
    await server.tool("ingest_commit", {
      sha: "failed-string-conversion",
      message:
        "Revert string conversion\n\nWHY: converting typed session tickets to strings fixed login but broke middleware security checks",
      changed_files: ["src/login.js", "src/middleware.js"],
      timestamp: 1_999_999_800,
    });
    await server.tool("record_architectural_decision", {
      decision_text:
        "DECISION: session tickets are objects {value, expiresAt}; strings must be rejected, expiresAt <= now is invalid, and auth/login/middleware stay typed end to end.",
      related_nodes: [
        "artifact:src/auth.js",
        "artifact:src/login.js",
        "artifact:src/middleware.js",
      ],
    });
  } finally {
    await server.stop();
  }
}

async function seedLodestar(directory, database, sessionId) {
  const server = await startMcp(
    lodestarExe,
    directory,
    {
      LODESTAR_DB: database,
      LODESTAR_AGENT: "eval-agent",
      LODESTAR_LLM_URL: "http://127.0.0.1:1/v1",
    },
    sessionId,
  );
  try {
    const goalResult = await server.tool("define_goal", {
      kind: "invariant",
      title: "Typed session tickets",
      statement:
        "Session tickets are objects {value, expiresAt}; strings are forbidden and expiresAt <= now is invalid across auth, login, and middleware.",
    });
    const goalId = findId(goalResult, ["goal_id", "id"]);
    if (!goalId)
      throw new Error(
        `could not find goal id in ${JSON.stringify(goalResult)}`,
      );
    await server.tool("link_goal_to_code", {
      goal_id: goalId,
      node_ids: [
        "artifact:src/auth.js",
        "artifact:src/login.js",
        "artifact:src/middleware.js",
      ],
      mode: "governed",
    });
    const taskResult = await server.tool("create_task", {
      goal_id: goalId,
      title: "Repair typed session validation regression",
      acceptance:
        "Valid unexpired object tickets pass; strings, empty values, and expiresAt <= now fail; login and middleware remain typed; report all impacted production files.",
    });
    const taskId = findId(taskResult, ["task_id", "id"]);
    if (!taskId)
      throw new Error(
        `could not find task id in ${JSON.stringify(taskResult)}`,
      );
    await server.tool("claim_task", {
      task_id: taskId,
      lease_secs: 3600,
    });
  } finally {
    await server.stop();
  }
}

function startMcp(command, cwd, extraEnv, sessionId) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, [], {
      cwd,
      env: { ...process.env, ...extraEnv },
      stdio: ["pipe", "pipe", "pipe"],
    });
    const pending = new Map();
    let nextId = 1;
    const stderr = [];
    child.stderr.on("data", (chunk) => stderr.push(chunk.toString()));
    child.on("error", reject);
    readline.createInterface({ input: child.stdout }).on("line", (line) => {
      let message;
      try {
        message = JSON.parse(line);
      } catch {
        return;
      }
      const handler = pending.get(message.id);
      if (handler) {
        pending.delete(message.id);
        message.error
          ? handler.reject(new Error(message.error.message))
          : handler.resolve(message.result);
      }
    });
    const request = (method, params) =>
      new Promise((resolveRequest, rejectRequest) => {
        const id = nextId++;
        pending.set(id, { resolve: resolveRequest, reject: rejectRequest });
        child.stdin.write(
          `${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`,
        );
      });
    const api = {
      async tool(name, args) {
        const response = await request("tools/call", {
          name,
          arguments: { ...args, session_id: sessionId },
        });
        if (response?.isError)
          throw new Error(response.content?.[0]?.text ?? `${name} failed`);
        return JSON.parse(response.content?.[0]?.text ?? "null");
      },
      async stop() {
        child.stdin.end();
        child.kill();
        await new Promise((done) =>
          child.exitCode === null ? child.once("exit", done) : done(),
        );
      },
    };
    request("initialize", {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "mindleak-agent-eval", version: "1" },
    })
      .then(async () => {
        await api.tool("open_session", {});
        resolve(api);
      })
      .catch((error) =>
        reject(new Error(`${error.message}\n${stderr.join("")}`)),
      );
  });
}

async function evaluateWorkspace(directory) {
  const failures = [];
  const now = 1_000;
  let hiddenPassed = 0;
  const check = (name, condition) => {
    if (condition) hiddenPassed += 1;
    else failures.push(name);
  };
  try {
    const nonce = `${Date.now()}-${Math.random()}`;
    const auth = await import(
      `${pathToFileURL(path.join(directory, "src/auth.js")).href}?${nonce}`
    );
    const login = await import(
      `${pathToFileURL(path.join(directory, "src/login.js")).href}?${nonce}`
    );
    const middleware = await import(
      `${pathToFileURL(path.join(directory, "src/middleware.js")).href}?${nonce}`
    );
    const valid = { value: "session-123", expiresAt: now + 1 };
    check("valid object ticket", auth.validateSession(valid, now) === true);
    check(
      "string rejected",
      auth.validateSession("session-123", now) === false,
    );
    check(
      "empty value rejected",
      auth.validateSession({ value: "", expiresAt: now + 1 }, now) === false,
    );
    check(
      "expiry boundary rejected",
      auth.validateSession({ value: "x", expiresAt: now }, now) === false,
    );
    check(
      "past expiry rejected",
      auth.validateSession({ value: "x", expiresAt: now - 1 }, now) === false,
    );
    check(
      "login remains typed",
      login.handleLogin({ ticket: valid }, now) === true,
    );
    let nextCalled = false;
    const response = {
      statusCode: 200,
      status(code) {
        this.statusCode = code;
      },
    };
    check(
      "middleware accepts valid",
      middleware.requireAuth(
        { ticket: valid },
        response,
        () => {
          nextCalled = true;
        },
        now,
      ) === true && nextCalled,
    );
    nextCalled = false;
    const denied = {
      statusCode: 200,
      status(code) {
        this.statusCode = code;
      },
    };
    check(
      "middleware rejects string",
      middleware.requireAuth(
        { ticket: "x" },
        denied,
        () => {
          nextCalled = true;
        },
        now,
      ) === false &&
        denied.statusCode === 401 &&
        !nextCalled,
    );
  } catch (error) {
    failures.push(`module error: ${error.message}`);
  }
  const hiddenTotal = 8;
  const publicTest = spawnSync(process.execPath, ["public-test.mjs"], {
    cwd: directory,
    encoding: "utf8",
    timeout: 30_000,
  });
  const protectedUnchanged = [...protectedHashes].every(
    ([file, expected]) =>
      fs.existsSync(path.join(directory, file)) &&
      hash(fs.readFileSync(path.join(directory, file))) === expected,
  );
  let impacted = [];
  try {
    const result = JSON.parse(
      fs.readFileSync(path.join(directory, "RESULT.json"), "utf8"),
    );
    impacted = Array.isArray(result.impacted_files)
      ? result.impacted_files
      : [];
  } catch {
    impacted = [];
  }
  const impactedSet = new Set(impacted);
  const truePositives = [...impactedSet].filter((file) =>
    impactTruth.has(file),
  ).length;
  const precision =
    impactedSet.size === 0 ? 0 : truePositives / impactedSet.size;
  const recall = truePositives / impactTruth.size;
  const f1 =
    precision + recall === 0
      ? 0
      : (2 * precision * recall) / (precision + recall);
  const taskSuccess =
    hiddenPassed === hiddenTotal &&
    publicTest.status === 0 &&
    protectedUnchanged &&
    f1 >= 0.8;
  return {
    task_success: taskSuccess,
    hidden_passed: hiddenPassed,
    hidden_total: hiddenTotal,
    regression_count:
      failures.length +
      (publicTest.status === 0 ? 0 : 1) +
      (protectedUnchanged ? 0 : 1),
    failures,
    public_test_passed: publicTest.status === 0,
    protected_files_unchanged: protectedUnchanged,
    impacted_files: {
      predicted: [...impactedSet].sort(),
      precision,
      recall,
      f1,
    },
  };
}

function summarizeEvents(events) {
  const starts = events.filter(
    (event) => event.type === "tool.execution_start",
  );
  const toolNames = starts.map((event) => event.data?.toolName ?? "unknown");
  const exploration = toolNames.filter((name) => {
    if (
      ["view", "grep", "glob", "shell", "bash", "powershell"].includes(name)
    ) {
      return true;
    }
    if (name.startsWith("mindleak-eval-")) {
      return /(graph_|impact_radius|recall|evidence_for)/.test(name);
    }
    if (name.startsWith("lodestar-eval-")) {
      return /(constitution|board|next_task|active_knowledge)/.test(name);
    }
    return false;
  });
  const outputTokens = events
    .filter((event) => event.type === "assistant.message")
    .reduce((sum, event) => sum + (event.data?.outputTokens ?? 0), 0);
  const final = [...events].reverse().find((event) => event.type === "result");
  return {
    model: events.find((event) => event.type === "session.tools_updated")?.data
      ?.model,
    total_tool_calls: toolNames.length,
    exploration_tool_calls: exploration.length,
    tool_names: toolNames,
    output_tokens: outputTokens,
    premium_requests: final?.usage?.premiumRequests ?? null,
    api_duration_ms: final?.usage?.totalApiDurationMs ?? null,
  };
}

function parseJsonLines(text) {
  return text
    .split(/\r?\n/)
    .filter(Boolean)
    .flatMap((line) => {
      try {
        return [JSON.parse(line)];
      } catch {
        return [];
      }
    });
}

function findId(value, keys) {
  if (!value || typeof value !== "object") return undefined;
  for (const key of keys) {
    if (typeof value[key] === "string") return value[key];
  }
  for (const child of Object.values(value)) {
    const found = findId(child, keys);
    if (found) return found;
  }
  return undefined;
}

function shuffled(values, seed) {
  const output = [...values];
  let state = seed >>> 0;
  for (let index = output.length - 1; index > 0; index -= 1) {
    state = (state * 1664525 + 1013904223) >>> 0;
    const selected = state % (index + 1);
    [output[index], output[selected]] = [output[selected], output[index]];
  }
  return output;
}

function median(values) {
  if (values.length === 0) return 0;
  const middle = Math.floor(values.length / 2);
  return values.length % 2 === 0
    ? (values[middle - 1] + values[middle]) / 2
    : values[middle];
}

function variance(values) {
  if (values.length === 0) return 0;
  const mean = values.reduce((sum, value) => sum + value, 0) / values.length;
  return (
    values.reduce((sum, value) => sum + (value - mean) ** 2, 0) / values.length
  );
}

function hash(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function prepareCopilotHome(target) {
  const source = path.join(os.homedir(), ".copilot");
  fs.mkdirSync(target, { recursive: true });
  for (const file of ["config.json", "m-encryption-key.enc"]) {
    const from = path.join(source, file);
    if (!fs.existsSync(from)) {
      throw new Error(`missing Copilot authentication state: ${from}`);
    }
    fs.copyFileSync(from, path.join(target, file));
  }
}
