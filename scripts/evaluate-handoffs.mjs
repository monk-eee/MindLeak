// Run the deterministic same-file coordination comparison.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

const root = process.cwd();
const build = spawnSync(
  "cargo",
  [
    "build",
    "--quiet",
    "--release",
    "--locked",
    "-p",
    "lodestar-core",
    "--example",
    "evaluate_handoffs",
  ],
  { cwd: root, encoding: "utf8" },
);
if (build.status !== 0) {
  process.stderr.write(build.stderr);
  process.exit(build.status ?? 1);
}
const executable = path.join(
  root,
  "target",
  "release",
  "examples",
  process.platform === "win32" ? "evaluate_handoffs.exe" : "evaluate_handoffs",
);
const run = spawnSync(executable, [], { cwd: root, encoding: "utf8" });
if (run.status !== 0) {
  process.stderr.write(run.stderr);
  process.exit(run.status ?? 1);
}
const result = JSON.parse(run.stdout);
const revision = spawnSync("git", ["rev-parse", "--short", "HEAD"], {
  cwd: root,
  encoding: "utf8",
}).stdout.trim();
const dirty =
  spawnSync("git", ["status", "--porcelain"], {
    cwd: root,
    encoding: "utf8",
  }).stdout.trim().length > 0;
result.source_revision = dirty ? `${revision}-dirty` : revision;
const sourceInputs = [
  "Cargo.toml",
  "Cargo.lock",
  "crates/lodestar-core/Cargo.toml",
  ...walkFiles(path.join(root, "crates", "lodestar-core", "src")),
  "crates/lodestar-core/examples/evaluate_handoffs.rs",
  "scripts/evaluate-handoffs.mjs",
].map((file) =>
  path.relative(root, path.resolve(root, file)).replaceAll("\\", "/"),
);
const sourceHash = crypto.createHash("sha256");
for (const file of [...new Set(sourceInputs)].sort()) {
  const bytes = fs.readFileSync(path.join(root, file));
  sourceHash.update(`${file.length}:${file}:${bytes.length}:`);
  sourceHash.update(bytes);
}
result.source_sha256 = sourceHash.digest("hex");
result.build_instance_sha256 = crypto
  .createHash("sha256")
  .update(fs.readFileSync(executable))
  .digest("hex");
result.build = {
  profile: "release",
  locked: true,
  rustc: commandVersion("rustc"),
  cargo: commandVersion("cargo"),
};
result.provenance = {
  authoritative: [
    "source_sha256",
    "build.profile",
    "build.locked",
    "build.rustc",
    "build.cargo",
    "passed",
  ],
  build_instance_sha256:
    "Informational digest of this PE/ELF/Mach-O build instance; not required to reproduce across linker invocations.",
};
result.passed =
  result.independent_tasks.concurrent_claims === 2 &&
  result.independent_tasks.same_artifact === true &&
  result.independent_tasks.concurrent_same_file_ownership_risk === true &&
  result.progressive_handoff.early_second_claim === false &&
  result.progressive_handoff.first_completed === true &&
  result.progressive_handoff.completion_verdict === "aligned" &&
  result.progressive_handoff.successor_status_after_completion === "open" &&
  result.progressive_handoff.successor_dependency_cleared === true &&
  result.progressive_handoff.synthetic_conformance_evidence === true &&
  result.progressive_handoff.second_claimed_after_handoff === true &&
  result.progressive_handoff.max_concurrent_claims === 1 &&
  result.progressive_handoff.concurrent_same_file_ownership_risk === false;

const output = `${JSON.stringify(result, null, 2)}\n`;
if (process.argv.includes("--write")) {
  const destination = path.join(
    root,
    "benchmarks",
    "results",
    "2026-07-23-progressive-handoff.json",
  );
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  fs.writeFileSync(destination, output);
}
process.stdout.write(output);
if (!result.passed) {
  process.exitCode = 1;
}

function walkFiles(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const child = path.join(directory, entry.name);
    return entry.isDirectory() ? walkFiles(child) : [child];
  });
}

function commandVersion(command) {
  const version = spawnSync(command, ["--version"], {
    cwd: root,
    encoding: "utf8",
  });
  if (version.status !== 0) {
    throw new Error(`${command} --version failed: ${version.stderr}`);
  }
  return version.stdout.trim();
}
