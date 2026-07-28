// Tests for the module length measurement. Run with: make script-test
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { measure, sourceLines, THRESHOLD } from "./measure-module-length.mjs";

const fixture = (files) => {
  const root = mkdtempSync(path.join(tmpdir(), "modlen-"));
  for (const [rel, body] of Object.entries(files)) {
    const full = path.join(root, rel);
    mkdirSync(path.dirname(full), { recursive: true });
    writeFileSync(full, body, "utf8");
  }
  return root;
};

const lines = (n, text = "let x = 1;") =>
  Array.from({ length: n }, () => text).join("\n");

// A well-tested module is not a long module. Counting colocated tests would
// penalise exactly the modules we most want people to keep writing.
test("a module is measured above its test block, not through it", () => {
  const body = `${lines(10)}\n#[cfg(test)]\nmod tests {\n${lines(500)}\n}\n`;
  assert.equal(sourceLines(body), 10);
});

test("a module with no test block is measured whole", () => {
  assert.equal(sourceLines(lines(30)), 30);
});

test("`mod tests` without the cfg attribute still ends the count", () => {
  assert.equal(sourceLines(`${lines(7)}\nmod tests {\n${lines(90)}\n}\n`), 7);
});

// `mod testsomething` is a real module, not the test block.
test("a module whose name merely starts with tests is not the test block", () => {
  const body = `${lines(4)}\nmod testsupport;\n${lines(6)}`;
  assert.equal(sourceLines(body), 11);
});

test("integration suites and tests.rs modules are not governed", () => {
  const root = fixture({
    "core/src/real.rs": lines(20),
    "core/tests/integration.rs": lines(900),
    "core/src/graph/tests.rs": lines(900),
    "core/src/store/design_tests.rs": lines(900),
  });
  try {
    const measured = measure(root);
    assert.deepEqual(
      measured.map((m) => m.path),
      ["core/src/real.rs"],
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("only Rust sources are counted", () => {
  const root = fixture({
    "core/src/real.rs": lines(12),
    "core/src/notes.md": lines(900),
    "core/src/build.mjs": lines(900),
  });
  try {
    assert.deepEqual(
      measure(root).map((m) => m.path),
      ["core/src/real.rs"],
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

// The baseline is a count of modules *over* the line. Off-by-one here would
// silently move the baseline, which is the one number the ratchet trusts.
test("the threshold is exclusive: at the line is not over it", () => {
  const root = fixture({
    "a/src/at.rs": lines(THRESHOLD),
    "a/src/over.rs": lines(THRESHOLD + 1),
    "a/src/under.rs": lines(THRESHOLD - 1),
  });
  try {
    const over = measure(root).filter((m) => m.lines > THRESHOLD);
    assert.deepEqual(
      over.map((m) => m.path),
      ["a/src/over.rs"],
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("modules are reported longest first", () => {
  const root = fixture({
    "a/src/small.rs": lines(5),
    "a/src/big.rs": lines(50),
    "a/src/mid.rs": lines(20),
  });
  try {
    assert.deepEqual(
      measure(root).map((m) => m.path),
      ["a/src/big.rs", "a/src/mid.rs", "a/src/small.rs"],
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("paths are reported with forward slashes on every OS", () => {
  const root = fixture({ "a/src/deep/nested.rs": lines(3) });
  try {
    assert.deepEqual(
      measure(root).map((m) => m.path),
      ["a/src/deep/nested.rs"],
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
