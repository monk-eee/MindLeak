import { execFileSync, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";

const workspace = process.cwd();
const allowFailures = process.argv.includes("--allow-failures");
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
const revision = execFileSync("git", ["rev-parse", "--short", "HEAD"], {
  cwd: workspace,
  encoding: "utf8",
}).trim();
const dirty =
  execFileSync("git", ["status", "--porcelain"], {
    cwd: workspace,
    encoding: "utf8",
  }).trim().length > 0;
const executableSha256 = createHash("sha256")
  .update(fs.readFileSync(executable))
  .digest("hex");
const tempDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "mindleak-eval-"));
const database = path.join(tempDirectory, "graph.db");
const serverEnvironment = { ...process.env, MINDLEAK_DB: database };
delete serverEnvironment.MINDLEAK_AGENT;
const server = spawn(executable, [], {
  cwd: workspace,
  env: serverEnvironment,
  stdio: ["pipe", "pipe", "pipe"],
});

let nextId = 1;
const pending = new Map();
let result;
const lines = readline.createInterface({ input: server.stdout });
lines.on("line", (line) => {
  const message = JSON.parse(line);
  const completion = pending.get(message.id);
  if (completion) {
    pending.delete(message.id);
    completion(message);
  }
});

function request(method, params) {
  const id = nextId++;
  server.stdin.write(
    `${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`,
  );
  return new Promise((resolve) => pending.set(id, resolve));
}

async function callTool(name, arguments_) {
  const response = await request("tools/call", { name, arguments: arguments_ });
  if (response.error || response.result?.isError) {
    throw new Error(JSON.stringify(response.error ?? response.result));
  }
  return JSON.parse(response.result.content[0].text);
}

try {
  const initialization = await request("initialize", {});

  await callTool("ingest_file", {
    path: "src/stale.ts",
    content:
      "export function caller() { removed(); }\nexport function removed() {}\n",
  });
  await callTool("ingest_file", {
    path: "src/stale.ts",
    content: "export function caller() {}\n",
  });
  const staleQuery = await callTool("graph_multi_hop_query", {
    seed_entity: "artifact:src/stale.ts",
    max_depth: 2,
    min_weight: 0,
  });
  const staleContainsPresent = staleQuery.edges.some(
    (edge) =>
      edge.relation === "contains" &&
      edge.target_id === "symbol:src/stale.ts:removed",
  );
  const staleCallPresent = staleQuery.edges.some(
    (edge) =>
      edge.relation === "calls" &&
      edge.source_id === "symbol:src/stale.ts:caller" &&
      edge.target_id === "symbol:src/stale.ts:removed",
  );
  const staleExactQuery = await callTool("graph_multi_hop_query", {
    seed_entity: "symbol:src/stale.ts:removed",
    max_depth: 1,
    min_weight: 0,
  });
  const staleSymbolPresent = staleExactQuery.nodes.some(
    (node) => node.id === "symbol:src/stale.ts:removed",
  );
  const staleFtsQuery = await callTool("graph_multi_hop_query", {
    seed_entity: "removed",
    max_depth: 1,
    min_weight: 0,
  });
  const staleFtsEntryPresent = staleFtsQuery.nodes.some(
    (node) => node.id === "symbol:src/stale.ts:removed",
  );

  await callTool("ingest_file", {
    path: "src/dependency.ts",
    content: "export function dependency() {}\n",
  });
  await callTool("ingest_file", {
    path: "src/consumer.ts",
    content:
      'import { dependency } from "./dependency";\nexport function consumer() { dependency(); }\n',
  });
  const impact = await callTool("get_impact_radius", {
    target_artifact: "artifact:src/dependency.ts",
  });
  const dependentArtifactPresent = impact.nodes.some(
    (node) => node.id === "artifact:src/consumer.ts",
  );
  const structuralDependencyPresent = impact.edges.some(
    (edge) =>
      edge.relation === "imports" &&
      edge.source_id === "artifact:src/consumer.ts" &&
      edge.target_id === "artifact:src/dependency.ts",
  );

  result = {
    schema_version: 2,
    source_revision: dirty ? `${revision}-dirty` : revision,
    executable_sha256: executableSha256,
    server_version: initialization.result.serverInfo.version,
    scenarios: {
      stale_structure_retraction: {
        expected_old_structure_present: false,
        observed_old_contains_present: staleContainsPresent,
        observed_old_call_present: staleCallPresent,
        observed_old_symbol_present: staleSymbolPresent,
        observed_old_fts_entry_present: staleFtsEntryPresent,
        passed:
          !staleContainsPresent &&
          !staleCallPresent &&
          !staleSymbolPresent &&
          !staleFtsEntryPresent,
      },
      cross_file_impact: {
        expected_dependent_artifact_present: true,
        expected_structural_dependency_present: true,
        observed_dependent_artifact_present: dependentArtifactPresent,
        observed_structural_dependency_present: structuralDependencyPresent,
        passed: dependentArtifactPresent && structuralDependencyPresent,
      },
    },
  };
  console.log(JSON.stringify(result, null, 2));
} finally {
  server.stdin.end();
  await new Promise((resolve) => {
    if (server.exitCode !== null) {
      resolve();
      return;
    }
    server.once("close", resolve);
    server.kill();
  });
  fs.rmSync(tempDirectory, { recursive: true, force: true });
}

if (
  !allowFailures &&
  Object.values(result.scenarios).some((scenario) => !scenario.passed)
) {
  process.exitCode = 1;
}
