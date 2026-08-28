// Guard: a shipped MCP server must be able to do what its own error message
// tells the operator to do. Run with: make script-test
//
// The defect this exists to prevent, measured on 2026-08-28:
// `CoordinationModeError::NoFederationClient` refuses a `federated`
// coordination mode with the remedy "install a build that includes the
// client". Every release had been built as
// `cargo build --release --locked -p mindleak-mcp -p lodestar-mcp` with no
// `--features`, so no published binary ever included that client. The
// federated claim path was written, reviewed, tested against real PostgreSQL,
// and compiled into nothing a user could obtain -- and the remedy named a
// thing that did not exist.
//
// The invariant is deliberately about *every* build site rather than one
// pinned line: the failure mode is a new workflow step that builds the servers
// and quietly omits the feature, which is exactly how the original gap
// survived two workflows at once.

import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const WORKFLOWS = ["release.yml", "ci.yml"];
const FEATURE = "federation-client";

/**
 * Every `run:` block in `workflow` that cargo-builds the MCP server binaries.
 *
 * Steps are split on `- name:` so a block's own continuation lines stay with
 * it; a `>-` folded `run:` spans several lines and must be judged whole.
 */
function serverBuildSteps(workflow) {
  return workflow
    .split(/^\s*-\s+name:/m)
    .filter(
      (step) =>
        step.includes("cargo build") &&
        step.includes("-p mindleak-mcp") &&
        step.includes("-p lodestar-mcp"),
    );
}

test("every workflow that builds the MCP servers enables the federation client", () => {
  for (const name of WORKFLOWS) {
    const workflow = fs.readFileSync(
      path.join(repoRoot, ".github", "workflows", name),
      "utf8",
    );
    const steps = serverBuildSteps(workflow);

    assert.ok(
      steps.length > 0,
      `${name} has no MCP server build step -- the parser is broken, or the ` +
        `build moved and this guard is now watching nothing`,
    );

    for (const step of steps) {
      assert.ok(
        step.includes(FEATURE),
        `${name} builds mindleak-mcp/lodestar-mcp without --features ` +
          `${FEATURE}. A binary built this way refuses ` +
          `MINDLEAK_COORDINATION_MODE=federated with NoFederationClient, whose ` +
          `remedy is to install a build that includes the client -- so shipping ` +
          `one makes that remedy unobtainable.`,
      );
    }
  }
});

test("the refusal names a remedy a user can actually reach", () => {
  const source = fs.readFileSync(
    path.join(repoRoot, "crates", "ackplane-core", "src", "lib.rs"),
    "utf8",
  );
  const start = source.indexOf("NoFederationClient,");
  assert.ok(start > 0, "NoFederationClient variant not found");
  // The attribute sits above the variant; take the preceding block.
  const message = source.slice(Math.max(0, start - 900), start);

  assert.ok(
    /release/i.test(message),
    "the refusal must point at released binaries, which now carry the client, " +
      "rather than an unqualified 'install a build that includes the client' " +
      "that named something no release contained",
  );
  assert.ok(
    message.includes(FEATURE),
    `the refusal must name --features ${FEATURE} for a source build, since ` +
      "that is the only other way to reach a federation-capable binary",
  );
});
