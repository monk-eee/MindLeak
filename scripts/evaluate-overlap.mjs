// Run the deterministic two-agent pre-flight overlap evaluation (ADR-0024).

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
    "evaluate_overlap",
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
  process.platform === "win32" ? "evaluate_overlap.exe" : "evaluate_overlap",
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
  "crates/mindleak-core/Cargo.toml",
  ...walkFiles(path.join(root, "crates", "lodestar-core", "src")),
  ...walkFiles(path.join(root, "crates", "mindleak-core", "src")),
  "crates/lodestar-core/examples/evaluate_overlap.rs",
  "scripts/evaluate-overlap.mjs",
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

const aware = result.overlap_aware;
result.passed =
  result.control.preflight_check_run === false &&
  result.control.concurrent_claims === 2 &&
  result.control.same_path_concurrent_ownership_risk === true &&
  aware.claim_overlaps.length === 1 &&
  aware.claim_overlaps[0].owner === "alice" &&
  aware.claim_overlaps[0].matching_paths.includes("src/lib.rs") &&
  aware.footprint_overlaps.length === 1 &&
  aware.footprint_overlaps[0].agent_id === "agent:alice" &&
  aware.footprint_overlaps[0].relation === "modified" &&
  aware.decay_control.footprint_overlaps.length === 0 &&
  aware.checks_read_only === true &&
  aware.steer.applied === true &&
  aware.steer.second_status === "blocked" &&
  aware.steer.second_claimed_after_steer === false &&
  aware.steer.concurrent_claims_after_steer === 1 &&
  aware.steer.same_path_concurrent_ownership_risk === false;

const output = `${JSON.stringify(result, null, 2)}\n`;
if (process.argv.includes("--write")) {
  const destination = path.join(
    root,
    "benchmarks",
    "results",
    "2026-07-23-two-agent-overlap.json",
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
