// Conformance gate (ADR-0031): fail CI when changed, governed code has no aligned
// conformance receipt covering it. Reads the committed manifest artifact produced
// by `export_conformance_manifest` ({ governed_nodes, receipts:[{verdict,
// covered_nodes}] } — the intent plane's proof-of-work) rather than the local,
// gitignored `.lodestar/spec.db`, so it runs in CI where the DB is absent.
// Documentation nodes are exempt exactly as at conformance read time.
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
 *
 * It also returns what it was able to check. `ok` alone cannot distinguish
 * "every governed change is proven" from "nothing you changed was governed",
 * and on this repository it is nearly always the second: measured 2026-07-29,
 * the constitution binds 8 code nodes and none of them are in `crates/`, so a
 * pull request touching fifty Rust files passes this gate having inspected
 * none of them. That is the same shape as a conformance receipt that is
 * `aligned` over an empty bundle — agreement about nothing, reported in the
 * same words as proof. The caller is given the numbers so it can say which one
 * it means.
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
  let inScope = 0;
  let ungoverned = 0;
  for (const path of changedPaths) {
    if (isDocumentationNode(path)) {
      continue;
    }
    const id = toArtifactId(path);
    if (!governed.has(id)) {
      ungoverned += 1;
      continue;
    }
    inScope += 1;
    if (!covered.has(id)) {
      violations.push({
        node: id,
        reason: "governed code changed without an aligned conformance receipt",
      });
    }
  }
  return {
    ok: violations.length === 0,
    violations,
    // How much of this change the constitution actually had an opinion about.
    coverage: {
      inScope,
      ungoverned,
      governedNodes: governed.size,
      checkedAnything: inScope > 0,
    },
  };
}

/**
 * Governed ids that name no file in the working tree.
 *
 * Splitting, renaming, or deleting a governed file moves the code and leaves
 * the binding pointing at a path that no longer exists. Nothing fails when that
 * happens: the constitution simply stops governing the code, `advise` finds no
 * clauses for the new paths, and the loss is invisible because an orphaned
 * binding looks exactly like code that was never governed in the first place.
 *
 * Measured on this repository after a refactor campaign: 7 governed ids named
 * files that no longer existed — `graph.rs`, `graph/query.rs`, `graph/signal.rs`
 * and both `tools.rs` had all been split into directories, and not one of the
 * resulting modules inherited the binding.
 *
 * `exists` is injected so the rule is testable without a working tree.
 */
export function danglingBindings(artifact, exists) {
  return (artifact.governed_nodes ?? [])
    .filter((node) => !isDocumentationNode(node))
    .map((node) => node.replace(/^artifact:/, ""))
    .filter((path) => !exists(path))
    .sort();
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
  const { ok, violations, coverage } = evaluateGate(artifact, changed);

  // Reported whatever the gate decides: a binding that names nothing is not a
  // missing receipt, it is governance that has quietly stopped applying, and
  // the gate above cannot see it — an orphaned id never appears in a diff.
  const dangling = danglingBindings(artifact, (path) => fs.existsSync(path));
  if (dangling.length) {
    console.error(
      `conformance-gate: ${dangling.length} governed binding(s) name no file; that code is no longer governed:`,
    );
    for (const path of dangling) {
      console.error(`  - ${path}`);
    }
    console.error(
      "conformance-gate: rebind with link_goal_to_artifact, or unlink if the code is gone.",
    );
  }

  if (ok) {
    // Say what was inspected, not just that nothing failed. "No governed gaps"
    // over an empty scope is the gate agreeing about nothing, and it used to
    // print in the same words as a real pass.
    if (coverage.checkedAnything) {
      console.log(
        `conformance-gate: OK — ${coverage.inScope} governed change(s) covered by an aligned receipt, ` +
          `${coverage.ungoverned} outside the constitution, of ${changed.length} changed path(s).`,
      );
    } else {
      console.log(
        `conformance-gate: CHECKED NOTHING — none of ${coverage.ungoverned} changed code path(s) ` +
          `are governed (the constitution binds ${coverage.governedNodes} node(s)). ` +
          "This is not a pass; there was nothing in scope to verify.",
      );
    }
    if (dangling.length && options.strict) {
      process.exit(1);
    }
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
