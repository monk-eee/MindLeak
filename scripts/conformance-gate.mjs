// Conformance gate (ADR-0031): fail CI when changed, governed code has no aligned
// conformance receipt covering it. Reads the committed evidence artifact produced
// by `export_evidence` (portable proof — the intent plane's proof-of-work) rather
// than the local, gitignored `.lodestar/spec.db`, so it runs in CI where the DB is
// absent. Documentation nodes are exempt exactly as at conformance read time.
//
// Cross-platform, dependency-free Node (toolchain rule). Advisory by default; pass
// --strict to make violations fail the build (a ratchet, not a cliff).
//
// Usage:
//   node scripts/conformance-gate.mjs --artifact .lodestar/evidence/all.json \
//     --base origin/main [--strict]
//   node scripts/conformance-gate.mjs --artifact <f> --changed "a.rs,b.rs" [--strict]

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import process from "node:process";

/**
 * A code node is anything that is not documentation. Mirrors the Rust
 * `is_documentation_node`: markdown and the root LICENSE / CODEOWNERS files never
 * drive a conformance verdict, so a change to them never needs a receipt.
 */
export function isDocumentationNode(path) {
  const clean = path.replace(/^artifact:/, "");
  const file = clean.split("/").pop() ?? clean;
  return (
    clean.toLowerCase().endsWith(".md") ||
    file === "LICENSE" ||
    file === "CODEOWNERS"
  );
}

/** Normalise a repo path to a MindLeak artifact id. */
function toArtifactId(path) {
  return path.startsWith("artifact:")
    ? path
    : `artifact:${path.replace(/\\/g, "/")}`;
}

/**
 * Pure gate evaluation. `artifact` is the parsed evidence export:
 * `{ governed_nodes: string[], receipts: [{ verdict, token, covered_nodes }] }`.
 * `changedPaths` are repo-relative paths from the PR. Returns the violations:
 * changed, governed, non-doc nodes with no covering `aligned` receipt.
 */
export function evaluateGate(artifact, changedPaths) {
  const governed = new Set(artifact.governed_nodes ?? []);
  const covered = new Set();
  for (const receipt of artifact.receipts ?? []) {
    if (receipt.verdict === "aligned") {
      for (const node of receipt.covered_nodes ?? []) {
        covered.add(node);
      }
    }
  }

  const violations = [];
  for (const path of changedPaths) {
    if (isDocumentationNode(path)) {
      continue;
    }
    const id = toArtifactId(path);
    if (governed.has(id) && !covered.has(id)) {
      violations.push({
        node: id,
        reason: "governed code changed without an aligned conformance receipt",
      });
    }
  }
  return { ok: violations.length === 0, violations };
}

function parseArguments(argv) {
  const options = { artifact: null, base: null, changed: null, strict: false };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--strict") {
      options.strict = true;
    } else if (["--artifact", "--base", "--changed"].includes(argument)) {
      const value = argv[index + 1];
      if (!value || value.startsWith("--")) {
        throw new Error(`${argument} requires a value`);
      }
      options[argument.slice(2)] = value;
      index += 1;
    } else {
      throw new Error(`unknown argument: ${argument}`);
    }
  }
  if (!options.artifact) {
    throw new Error("--artifact <path> is required");
  }
  return options;
}

/** Resolve the PR's changed paths from an explicit list or a git diff against base. */
function resolveChangedPaths(options) {
  if (options.changed) {
    return options.changed
      .split(/[\n,]/)
      .map((value) => value.trim())
      .filter(Boolean);
  }
  const base = options.base ?? "origin/main";
  const output = execFileSync(
    "git",
    ["diff", "--name-only", `${base}...HEAD`],
    {
      encoding: "utf8",
    },
  );
  return output.split(/\r?\n/).filter(Boolean);
}

function main() {
  let options;
  try {
    options = parseArguments(process.argv.slice(2));
  } catch (error) {
    console.error(`conformance-gate: ${error.message}`);
    process.exit(2);
  }

  const artifact = JSON.parse(fs.readFileSync(options.artifact, "utf8"));
  const changed = resolveChangedPaths(options);
  const { ok, violations } = evaluateGate(artifact, changed);

  if (ok) {
    console.log(
      `conformance-gate: OK — ${changed.length} changed path(s), no governed gaps.`,
    );
    return;
  }

  console.error(
    `conformance-gate: ${violations.length} governed change(s) lack an aligned receipt:`,
  );
  for (const violation of violations) {
    console.error(`  - ${violation.node}: ${violation.reason}`);
  }
  if (options.strict) {
    process.exit(1);
  }
  console.error(
    "conformance-gate: advisory mode (pass --strict to fail the build).",
  );
}

// Only run the CLI when invoked directly, so the pure functions stay importable.
if (
  import.meta.url === `file://${process.argv[1]}` ||
  process.argv[1]?.endsWith("conformance-gate.mjs")
) {
  main();
}
