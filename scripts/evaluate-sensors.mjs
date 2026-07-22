// Reproducible passive-sensor processing benchmark.
// Measures the shipping in-process privacy/path pipeline plus local MCP ingest.

import crypto from "node:crypto";
import fs from "node:fs";
import { createRequire } from "node:module";
import { performance } from "node:perf_hooks";
import { execFileSync } from "node:child_process";

import { driveServer, resolveExe } from "./experiments/harness.mjs";

const root = process.cwd();
const require = createRequire(import.meta.url);
const {
  filterChangedPaths,
  redactTerminalOutput,
} = require("../editors/vscode/out/util.js");
const executable = resolveExe(root);
const revision = execFileSync("git", ["rev-parse", "--short", "HEAD"], {
  cwd: root,
  encoding: "utf8",
}).trim();
const dirty =
  execFileSync("git", ["status", "--porcelain"], {
    cwd: root,
    encoding: "utf8",
  }).trim().length > 0;
const executableSha256 = crypto
  .createHash("sha256")
  .update(fs.readFileSync(executable))
  .digest("hex");
const { request, tool, cleanup } = driveServer(executable, root, {
  MINDLEAK_LOG: "off",
});

const samples = [];
const rawPaths = Array.from({ length: 260 }, (_, index) =>
  index < 40 ? `target/generated-${index}` : `src/file-${index}.ts`,
);
const rawOutput = `${"build output\n".repeat(300)}token=secret-value`;

try {
  await request("initialize", {});
  for (let index = 0; index < 60; index += 1) {
    const started = performance.now();
    const changedFiles = filterChangedPaths(rawPaths, ["target"], 200);
    const output = redactTerminalOutput(rawOutput, 8192);
    await tool("ingest_execution", {
      command: `sensor-benchmark-${index}`,
      exit_code: 0,
      output,
      cwd: root,
      changed_files: changedFiles,
      timestamp: 2_000_000_000 + index,
    });
    if (index >= 10) {
      samples.push(performance.now() - started);
    }
  }
} finally {
  await cleanup();
}

samples.sort((left, right) => left - right);
const percentile = (fraction) =>
  samples[Math.ceil(samples.length * fraction) - 1];
const result = {
  schema_version: 1,
  source_revision: dirty ? `${revision}-dirty` : revision,
  executable_sha256: executableSha256,
  samples: samples.length,
  changed_files_per_sample: 200,
  retained_output_chars: 8192,
  p50_ms: Number(percentile(0.5).toFixed(3)),
  p95_ms: Number(percentile(0.95).toFixed(3)),
  max_ms: Number(samples.at(-1).toFixed(3)),
  gate_ms: 50,
};
result.passed = result.p95_ms < result.gate_ms;
console.log(JSON.stringify(result, null, 2));
if (!result.passed) {
  process.exitCode = 1;
}
