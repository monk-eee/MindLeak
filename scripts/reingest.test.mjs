// Tests for the re-ingest pass. Run with: make script-test
import assert from "node:assert/strict";
import { test } from "node:test";

import { isExtractable, isIgnoredPath, selectFiles } from "./reingest.mjs";

test("junk directories are skipped in any position", () => {
  assert.equal(isIgnoredPath("target/debug/build.rs"), true);
  assert.equal(isIgnoredPath("crates/x/target/y.rs"), true);
  assert.equal(isIgnoredPath("editors/vscode/node_modules/pkg/index.js"), true);
  assert.equal(isIgnoredPath(".lodestar/evidence/all.json"), true);
  assert.equal(isIgnoredPath("crates/mindleak-core/src/lib.rs"), false);
});

test("a directory merely starting with a junk name is not junk", () => {
  // `targets/` and `distribution/` are ordinary directories. Matching on a
  // prefix rather than a whole segment would silently drop real source.
  assert.equal(isIgnoredPath("targets/plan.rs"), false);
  assert.equal(isIgnoredPath("distribution/setup.py"), false);
});

test("only files the extractor understands are selected", () => {
  assert.equal(isExtractable("src/lib.rs"), true);
  assert.equal(isExtractable("editors/vscode/src/util.ts"), true);
  assert.equal(isExtractable("scripts/thing.mjs"), true);
  assert.equal(isExtractable("docs/SPEC.md"), false);
  assert.equal(isExtractable("assets/logo.png"), false);
});

test("manifests are selected for their dependency edges", () => {
  assert.equal(isExtractable("Cargo.toml"), true);
  assert.equal(isExtractable("crates/mindleak-core/Cargo.toml"), true);
  assert.equal(isExtractable("editors/vscode/package.json"), true);
  // A .toml that is not a manifest carries no dependency edges.
  assert.equal(isExtractable("rustfmt.toml"), false);
});

test("a dotfile with no extension is not read as one", () => {
  // `.gitignore` must not parse as extension "gitignore"; a naive split on "."
  // treats every dotfile as an extension and sends junk to the extractor.
  assert.equal(isExtractable(".gitignore"), false);
  assert.equal(isExtractable("editors/vscode/.eslintrc"), false);
  assert.equal(isExtractable("Makefile"), false);
});

test("selection is filtered, de-slashed and stable", () => {
  const selected = selectFiles([
    "crates\\mindleak-core\\src\\lib.rs",
    "target/debug/junk.rs",
    "docs/SPEC.md",
    "",
    "  Cargo.toml  ",
    "crates/mindleak-core/src/decay.rs",
  ]);
  assert.deepEqual(selected, [
    "Cargo.toml",
    "crates/mindleak-core/src/decay.rs",
    "crates/mindleak-core/src/lib.rs",
  ]);
});
