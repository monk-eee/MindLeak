// Run the production signal model against an adversarial in-memory graph.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

const root = process.cwd();
const run = spawnSync(
  "cargo",
  ["run", "--quiet", "-p", "mindleak-core", "--example", "evaluate_signal"],
  { cwd: root, encoding: "utf8" },
);
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
const executable = path.join(
  root,
  "target",
  "debug",
  "examples",
  process.platform === "win32" ? "evaluate_signal.exe" : "evaluate_signal",
);
result.source_revision = dirty ? `${revision}-dirty` : revision;
result.executable_sha256 = crypto
  .createHash("sha256")
  .update(fs.readFileSync(executable))
  .digest("hex");
result.passed =
  result.scenarios.same_session_spam.signal_multiplier === 1 &&
  !result.scenarios.same_session_spam.active &&
  result.scenarios.resolved_failure.signal_multiplier > 4 &&
  result.scenarios.resolved_failure.active &&
  result.scenarios.resolved_failure.evidence.consequence &&
  result.scenarios.resolved_failure.evidence.surprise &&
  result.scenarios.resolved_failure.evidence.source_diversity >= 3 &&
  !result.scenarios.eventual_decay.active &&
  result.scenarios.consolidation_handoff.contains_failure &&
  result.scenarios.consolidation_handoff.expired_failure_retained &&
  result.ablation.consequence > result.ablation.span_qualified_reinforcement &&
  result.ablation.source_diversity >
    result.ablation.span_qualified_reinforcement &&
  result.ablation.maximal <= 8;
console.log(JSON.stringify(result, null, 2));
if (!result.passed) {
  process.exitCode = 1;
}
