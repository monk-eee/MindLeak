#!/usr/bin/env node
// Measure Rust module length for the source-files-stay-small-and-cohesive
// clause.
//
// Reports how many source modules exceed the advisory threshold. The number
// is a prompt for a judgement, not a verdict: the clause says length is "an
// advisory signal resolved by human judgment", and a cohesive module may
// legitimately sit above the line. What the ratchet bound to this metric
// prevents is the count drifting upward unnoticed — a module crossing the
// threshold surfaces at review, where it is either split or the baseline is
// accepted, and accepting an attributed baseline is how the exception gets
// stated and justified rather than forgotten.
//
// Counting rules, chosen so the number means what it appears to mean:
//   - Only `.rs` files under crates/ — the clause governs Rust modules.
//   - Lines above the first `#[cfg(test)]` / `mod tests`, so colocated tests
//     do not make a well-tested module look bloated.
//   - Test files themselves are excluded outright (integration suites and
//     `tests.rs` modules legitimately grow, and splitting them pays nothing).
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const THRESHOLD = 450;

const isTestFile = (rel) => {
  const base = path.basename(rel);
  return (
    rel.includes("/tests/") || base === "tests.rs" || base.endsWith("_tests.rs")
  );
};

/** Non-test lines in one Rust source: everything above its test module. */
export function sourceLines(text) {
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i++) {
    if (/^\s*(#\[cfg\(test\)\]|mod tests\b)/.test(lines[i])) return i;
  }
  return lines.length;
}

/** Every governed module under `root`, longest first. */
export function measure(root) {
  const found = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.name.endsWith(".rs")) found.push(full);
    }
  };
  walk(root);

  return found
    .map((full) => ({
      path: path.relative(root, full).split(path.sep).join("/"),
      full,
    }))
    .filter((m) => !isTestFile(`/${m.path}`))
    .map((m) => ({
      path: m.path,
      lines: sourceLines(readFileSync(m.full, "utf8")),
    }))
    .sort((a, b) => b.lines - a.lines);
}

const thisFile = fileURLToPath(import.meta.url);
if (process.argv[1] && path.resolve(process.argv[1]) === thisFile) {
  const root = path.join(path.dirname(thisFile), "..", "crates");
  const modules = measure(root);
  const over = modules.filter((m) => m.lines > THRESHOLD);

  if (process.argv.includes("--json")) {
    console.log(
      JSON.stringify(
        { threshold: THRESHOLD, measured: over.length, over },
        null,
        2,
      ),
    );
  } else {
    console.log(
      `${over.length} of ${modules.length} modules exceed ${THRESHOLD} non-test lines`,
    );
    for (const m of over) {
      console.log(`  ${String(m.lines).padStart(5)}  crates/${m.path}`);
    }
  }
}
