// Tests for the merge-driver guard. Run with: node --test scripts/
//
// The regression: `merge=union` on CHANGELOG.md produced conflicts on GitHub in
// the one file the attribute promised to make conflict-free, because drivers
// only run in a local checkout.
import assert from "node:assert/strict";
import { test } from "node:test";

import { mergeDriverDeclarations } from "./merge-driver-guard.mjs";

test("the union driver that caused the phantom conflicts is caught", () => {
  const found = mergeDriverDeclarations("CHANGELOG.md merge=union\n");

  assert.equal(found.length, 1);
  assert.equal(found[0].driver, "union");
  assert.equal(found[0].number, 1);
});

test("any driver is caught, not just union", () => {
  // A custom driver is worse: GitHub cannot run it at all.
  const found = mergeDriverDeclarations("docs/adr/README.md merge=keepboth\n");
  assert.equal(found[0].driver, "keepboth");
});

test("a commented explanation is not a declaration", () => {
  // The file has to stay able to document why the rule exists; a guard that
  // forbids naming the thing it forbids makes its own history unwritable.
  const text = [
    "# `merge=union` does exactly that in a checkout, and nowhere else.",
    "* text=auto eol=lf",
  ].join("\n");

  assert.deepEqual(mergeDriverDeclarations(text), []);
});

test("a trailing comment does not hide a real declaration", () => {
  const found = mergeDriverDeclarations(
    "CHANGELOG.md merge=union # keep both\n",
  );
  assert.equal(found.length, 1);
  assert.equal(found[0].driver, "union");
});

test("ordinary attributes are left alone", () => {
  const text = [
    "* text=auto eol=lf",
    "*.rs        text eol=lf",
    "*.png       binary",
    "*.md        text eol=lf",
  ].join("\n");

  assert.deepEqual(mergeDriverDeclarations(text), []);
});

test("an attribute merely containing the word merge is not a driver", () => {
  // `-merge` unsets; `mergeable` is not `merge=`. Neither declares a driver.
  assert.deepEqual(mergeDriverDeclarations("notes/mergeable.md text\n"), []);
});

test("every declaration is reported, with its line number", () => {
  const text = [
    "* text=auto eol=lf",
    "CHANGELOG.md merge=union",
    "*.rs text eol=lf",
    "docs/adr/README.md merge=union",
  ].join("\n");

  const found = mergeDriverDeclarations(text);
  assert.deepEqual(
    found.map((entry) => entry.number),
    [2, 4],
  );
});
