// Tests for the ADR-0031 conformance gate. Run with: node --test scripts/
import assert from "node:assert/strict";
import { test } from "node:test";

import { evaluateGate, isDocumentationNode } from "./conformance-gate.mjs";

const artifact = {
  governed_nodes: [
    "artifact:crates/mindleak-mcp/src/main.rs",
    "artifact:crates/lodestar-mcp/src/main.rs",
  ],
  receipts: [
    {
      verdict: "aligned",
      token: "35a244ab",
      covered_nodes: ["artifact:crates/mindleak-mcp/src/main.rs"],
    },
    {
      verdict: "drift",
      token: "af2b7b46",
      covered_nodes: ["artifact:crates/lodestar-mcp/src/main.rs"],
    },
  ],
};

test("documentation nodes are exempt", () => {
  assert.equal(isDocumentationNode("docs/USAGE.md"), true);
  assert.equal(isDocumentationNode("artifact:README.md"), true);
  assert.equal(isDocumentationNode("LICENSE"), true);
  assert.equal(isDocumentationNode("CODEOWNERS"), true);
  assert.equal(isDocumentationNode("crates/mindleak-mcp/src/main.rs"), false);
});

test("an aligned receipt covers its governed node", () => {
  const result = evaluateGate(artifact, ["crates/mindleak-mcp/src/main.rs"]);
  assert.equal(result.ok, true);
  assert.equal(result.violations.length, 0);
});

test("governed code without an aligned receipt is a violation (a drift receipt does not cover)", () => {
  const result = evaluateGate(artifact, ["crates/lodestar-mcp/src/main.rs"]);
  assert.equal(result.ok, false);
  assert.equal(result.violations.length, 1);
  assert.equal(
    result.violations[0].node,
    "artifact:crates/lodestar-mcp/src/main.rs",
  );
});

test("changing a doc node never violates, even under governance", () => {
  const withDoc = {
    ...artifact,
    governed_nodes: [...artifact.governed_nodes, "artifact:docs/SPEC.md"],
  };
  const result = evaluateGate(withDoc, ["docs/SPEC.md"]);
  assert.equal(result.ok, true);
});

test("ungoverned code change is not a violation", () => {
  const result = evaluateGate(artifact, [
    "crates/mindleak-core/src/whatever.rs",
  ]);
  assert.equal(result.ok, true);
});
