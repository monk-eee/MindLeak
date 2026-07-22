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
  const crossFileCallPresent = impact.edges.some(
    (edge) =>
      edge.relation === "calls" &&
      edge.source_id === "symbol:src/consumer.ts:consumer" &&
      edge.target_id === "symbol:src/dependency.ts:dependency",
  );

  await callTool("ingest_file", {
    path: "src/sibling.ts",
    content: "export function sibling() {}\n",
  });
  await callTool("ingest_file", {
    path: "src/coimporter.ts",
    content:
      'import { dependency } from "./dependency";\nimport { sibling } from "./sibling";\nexport function useBoth() { dependency(); sibling(); }\n',
  });
  const dependencyImpact = await callTool("get_impact_radius", {
    target_artifact: "artifact:src/dependency.ts",
  });
  const coimportedSiblingPresent = dependencyImpact.nodes.some(
    (node) => node.id === "artifact:src/sibling.ts",
  );

  await callTool("ingest_file", {
    path: "src/false-calls.ts",
    content:
      'import { dependency } from "./dependency";\nexport function falseCalls() {\n  // dependency();\n  other.dependency();\n}\n',
  });
  const falseCallGraph = await callTool("graph_multi_hop_query", {
    seed_entity: "symbol:src/false-calls.ts:falseCalls",
    max_depth: 1,
    min_weight: 0,
  });
  const falseCrossFileCallPresent = falseCallGraph.edges.some(
    (edge) => edge.relation === "calls",
  );

  await callTool("ingest_file", {
    path: "src/mixed-consumer.tsx",
    content:
      'import { dependency } from "./dependency";\nexport function mixedConsumer() { dependency(); }\n',
  });
  const mixedGraph = await callTool("graph_multi_hop_query", {
    seed_entity: "artifact:src/mixed-consumer.tsx",
    max_depth: 1,
    min_weight: 0,
  });
  const mixedResolvedToKnownArtifact = mixedGraph.edges.some(
    (edge) =>
      edge.relation === "imports" &&
      edge.target_id === "artifact:src/dependency.ts",
  );
  const wrongMixedStub = await callTool("graph_multi_hop_query", {
    seed_entity: "artifact:src/dependency.tsx",
    max_depth: 1,
    min_weight: 0,
  });
  const wrongMixedStubPresent = wrongMixedStub.nodes.some(
    (node) => node.id === "artifact:src/dependency.tsx",
  );

  await callTool("ingest_file", {
    path: "src/stub-consumer.ts",
    content: 'import "./ghost";\n',
  });
  await callTool("ingest_file", {
    path: "src/stub-consumer.ts",
    content: "",
  });
  const removedStub = await callTool("graph_multi_hop_query", {
    seed_entity: "artifact:src/ghost.ts",
    max_depth: 1,
    min_weight: 0,
  });
  const removedStubPresent = removedStub.nodes.some(
    (node) => node.id === "artifact:src/ghost.ts",
  );

  await callTool("ingest_file", {
    path: "src/consumer-first.tsx",
    content:
      'import { lateDependency } from "./late-dependency";\nexport function consumerFirst() { lateDependency(); }\n',
  });
  await callTool("ingest_file", {
    path: "src/late-dependency.ts",
    content: "export function lateDependency() {}\n",
  });
  const consumerFirstGraph = await callTool("graph_multi_hop_query", {
    seed_entity: "artifact:src/consumer-first.tsx",
    max_depth: 2,
    min_weight: 0,
  });
  const consumerFirstMixedPromoted = consumerFirstGraph.edges.some(
    (edge) =>
      edge.relation === "imports" &&
      edge.target_id === "artifact:src/late-dependency.ts",
  );
  const consumerFirstCallPromoted = consumerFirstGraph.edges.some(
    (edge) =>
      edge.relation === "calls" &&
      edge.target_id === "symbol:src/late-dependency.ts:lateDependency",
  );

  await callTool("ingest_file", {
    path: "src/index-consumer.ts",
    content: 'import "./feature";\n',
  });
  await callTool("ingest_file", {
    path: "src/feature/index.ts",
    content: "export const value = 1;\n",
  });
  const indexGraph = await callTool("graph_multi_hop_query", {
    seed_entity: "artifact:src/index-consumer.ts",
    max_depth: 1,
    min_weight: 0,
  });
  const consumerFirstIndexPromoted = indexGraph.edges.some(
    (edge) =>
      edge.relation === "imports" &&
      edge.target_id === "artifact:src/feature/index.ts",
  );

  await callTool("ingest_file", {
    path: "src/explicit-consumer.ts",
    content: 'import "./explicit.js";\n',
  });
  await callTool("ingest_file", {
    path: "src/explicit.ts",
    content: "export const value = 1;\n",
  });
  const explicitGraph = await callTool("graph_multi_hop_query", {
    seed_entity: "artifact:src/explicit-consumer.ts",
    max_depth: 1,
    min_weight: 0,
  });
  const explicitJsPromoted = explicitGraph.edges.some(
    (edge) =>
      edge.relation === "imports" &&
      edge.target_id === "artifact:src/explicit.ts",
  );

  await callTool("ingest_file", {
    path: "src/require-scope.js",
    content:
      "function scoped(require) { require('ghost-package'); }\nconst real = require('real-package');\n",
  });
  const ghostPackage = await callTool("graph_multi_hop_query", {
    seed_entity: "package:ghost-package",
    max_depth: 1,
    min_weight: 0,
  });
  const realPackage = await callTool("graph_multi_hop_query", {
    seed_entity: "package:real-package",
    max_depth: 1,
    min_weight: 0,
  });
  const ghostPackagePresent = ghostPackage.nodes.some(
    (node) => node.id === "package:ghost-package",
  );
  const realPackagePresent = realPackage.nodes.some(
    (node) => node.id === "package:real-package",
  );
  await callTool("ingest_file", {
    path: "src/require-var.js",
    content:
      "function scoped() { if (condition) { var require = local; } require('ghost-var'); }\nrequire('real-var');\n",
  });
  const ghostVarPackage = await callTool("graph_multi_hop_query", {
    seed_entity: "package:ghost-var",
    max_depth: 1,
    min_weight: 0,
  });
  const realVarPackage = await callTool("graph_multi_hop_query", {
    seed_entity: "package:real-var",
    max_depth: 1,
    min_weight: 0,
  });
  const ghostVarPackagePresent = ghostVarPackage.nodes.some(
    (node) => node.id === "package:ghost-var",
  );
  const realVarPackagePresent = realVarPackage.nodes.some(
    (node) => node.id === "package:real-var",
  );

  await callTool("ingest_file", {
    path: "src/parser-controls.ts",
    content:
      'import { dependency } from "./dependency";\nfunction local() {}\nexport function localCaller() { local(); }\nexport function typedConsumer({ flag }): void { dependency(); }\nexport function shadowedConsumer() { const localValue = value, dependency = localValue; dependency(); }\n',
  });
  const parserGraph = await callTool("graph_multi_hop_query", {
    seed_entity: "artifact:src/parser-controls.ts",
    max_depth: 2,
    min_weight: 0,
  });
  const localCallPresent = parserGraph.edges.some(
    (edge) =>
      edge.relation === "calls" &&
      edge.source_id === "symbol:src/parser-controls.ts:localCaller" &&
      edge.target_id === "symbol:src/parser-controls.ts:local",
  );
  const typedImportedCallPresent = parserGraph.edges.some(
    (edge) =>
      edge.relation === "calls" &&
      edge.source_id === "symbol:src/parser-controls.ts:typedConsumer" &&
      edge.target_id === "symbol:src/dependency.ts:dependency",
  );
  const shadowedImportedCallPresent = parserGraph.edges.some(
    (edge) =>
      edge.relation === "calls" &&
      edge.source_id === "symbol:src/parser-controls.ts:shadowedConsumer" &&
      edge.target_id === "symbol:src/dependency.ts:dependency",
  );

  await callTool("ingest_file", {
    path: "src/typed-require.ts",
    content:
      "function scoped(require): void { require('ghost-typed'); }\nrequire('real-typed');\n",
  });
  const ghostTypedPackage = await callTool("graph_multi_hop_query", {
    seed_entity: "package:ghost-typed",
    max_depth: 1,
    min_weight: 0,
  });
  const realTypedPackage = await callTool("graph_multi_hop_query", {
    seed_entity: "package:real-typed",
    max_depth: 1,
    min_weight: 0,
  });
  const ghostTypedPackagePresent = ghostTypedPackage.nodes.some(
    (node) => node.id === "package:ghost-typed",
  );
  const realTypedPackagePresent = realTypedPackage.nodes.some(
    (node) => node.id === "package:real-typed",
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
        expected_cross_file_call_present: true,
        observed_dependent_artifact_present: dependentArtifactPresent,
        observed_structural_dependency_present: structuralDependencyPresent,
        observed_cross_file_call_present: crossFileCallPresent,
        passed:
          dependentArtifactPresent &&
          structuralDependencyPresent &&
          crossFileCallPresent,
      },
      impact_negative_controls: {
        expected_coimported_sibling_present: false,
        expected_false_cross_file_call_present: false,
        observed_coimported_sibling_present: coimportedSiblingPresent,
        observed_false_cross_file_call_present: falseCrossFileCallPresent,
        passed: !coimportedSiblingPresent && !falseCrossFileCallPresent,
      },
      resolution_and_stub_lifecycle: {
        expected_mixed_extension_target: "artifact:src/dependency.ts",
        expected_wrong_mixed_stub_present: false,
        expected_removed_stub_present: false,
        observed_mixed_extension_target_present: mixedResolvedToKnownArtifact,
        observed_wrong_mixed_stub_present: wrongMixedStubPresent,
        observed_removed_stub_present: removedStubPresent,
        passed:
          mixedResolvedToKnownArtifact &&
          !wrongMixedStubPresent &&
          !removedStubPresent,
      },
      consumer_first_reconciliation: {
        expected_mixed_extension_promoted: true,
        expected_cross_file_call_promoted: true,
        expected_index_module_promoted: true,
        expected_explicit_js_promoted_to_typescript: true,
        observed_mixed_extension_promoted: consumerFirstMixedPromoted,
        observed_cross_file_call_promoted: consumerFirstCallPromoted,
        observed_index_module_promoted: consumerFirstIndexPromoted,
        observed_explicit_js_promoted_to_typescript: explicitJsPromoted,
        passed:
          consumerFirstMixedPromoted &&
          consumerFirstCallPromoted &&
          consumerFirstIndexPromoted &&
          explicitJsPromoted,
      },
      require_scope: {
        expected_shadowed_package_present: false,
        expected_unshadowed_package_present: true,
        expected_var_shadowed_package_present: false,
        expected_var_unshadowed_package_present: true,
        observed_shadowed_package_present: ghostPackagePresent,
        observed_unshadowed_package_present: realPackagePresent,
        observed_var_shadowed_package_present: ghostVarPackagePresent,
        observed_var_unshadowed_package_present: realVarPackagePresent,
        passed:
          !ghostPackagePresent &&
          realPackagePresent &&
          !ghostVarPackagePresent &&
          realVarPackagePresent,
      },
      parser_scope_controls: {
        expected_local_call_present: true,
        expected_destructured_typed_imported_call_present: true,
        expected_second_declarator_shadowed_call_present: false,
        expected_typed_require_shadowed_package_present: false,
        expected_typed_require_outer_package_present: true,
        observed_local_call_present: localCallPresent,
        observed_destructured_typed_imported_call_present:
          typedImportedCallPresent,
        observed_second_declarator_shadowed_call_present:
          shadowedImportedCallPresent,
        observed_typed_require_shadowed_package_present:
          ghostTypedPackagePresent,
        observed_typed_require_outer_package_present: realTypedPackagePresent,
        passed:
          localCallPresent &&
          typedImportedCallPresent &&
          !shadowedImportedCallPresent &&
          !ghostTypedPackagePresent &&
          realTypedPackagePresent,
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
