// Tests for the markdown link checker. Run with: node scripts/script-tests.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { execFileSync } from "node:child_process";

import { treeSets, brokenLinksIn, checkRepo } from "./link-check.mjs";

const tree = treeSets([
  "README.md",
  "AGENTS.md",
  "docs/USAGE.md",
  "docs/adr/0001-a.md",
  "crates/core/src/graph/mod.rs",
  "editors/vscode/media/mindleak_logo.png",
  "gaps.d/note.md",
]);

test("a link to an existing file is not broken", () => {
  const broken = brokenLinksIn(
    "README.md",
    "See [usage](docs/USAGE.md).",
    tree,
  );
  assert.equal(broken.length, 0);
});

test("a link to an existing directory is not broken", () => {
  const broken = brokenLinksIn(
    "DEVELOPERS.md",
    "The [gaps](gaps.d/) live here.",
    tree,
  );
  assert.equal(broken.length, 0);
});

test("a link to a missing file is broken, and names its location", () => {
  const broken = brokenLinksIn(
    "docs/USAGE.md",
    "gone: [x](../crates/core/src/graph.rs)",
    tree,
  );
  assert.equal(broken.length, 1);
  assert.equal(broken[0].file, "docs/USAGE.md");
  assert.equal(broken[0].line, 1);
  assert.match(broken[0].target, /graph\.rs/);
});

test("a root-relative target that exists from the repo root is accepted", () => {
  // From docs/USAGE.md this does not resolve file-relative, but the repo mixes
  // conventions and it resolves from root; flagging that would be noise.
  const broken = brokenLinksIn(
    "docs/USAGE.md",
    "[facade](crates/core/src/graph/mod.rs)",
    tree,
  );
  assert.equal(broken.length, 0);
});

test("a #anchor is stripped before the path is resolved", () => {
  const ok = brokenLinksIn("README.md", "[a](docs/USAGE.md#section)", tree);
  assert.equal(ok.length, 0);
  const bad = brokenLinksIn("README.md", "[a](docs/MISSING.md#section)", tree);
  assert.equal(bad.length, 1);
});

test("external, pure-anchor and placeholder links are not checked", () => {
  const text = [
    "[web](https://example.com)",
    "[mail](mailto:x@example.com)",
    "[top](#heading)",
    "[tpl](docs/${name}.md)",
    "[ellipsis](...)",
    "[cmd](a file with spaces.md)",
  ].join("\n");
  assert.equal(brokenLinksIn("README.md", text, tree).length, 0);
});

test("a pending screenshot image is exempt even when the file is absent", () => {
  const text = "![hero](editors/vscode/media/screenshots/overview.png)";
  assert.equal(brokenLinksIn("editors/vscode/README.md", text, tree).length, 0);
});

test("treeSets derives every ancestor directory of a tracked file", () => {
  const { dirSet } = tree;
  assert.ok(dirSet.has("crates/core/src/graph"));
  assert.ok(dirSet.has("crates"));
  assert.ok(dirSet.has("docs/adr"));
});

// The guard, live: this runs from pre-push via script-tests, so a living doc
// that starts pointing at a moved or deleted file fails the push rather than
// rotting unnoticed. `docs/adr/` is out of scope by design (see link-check.mjs).
test("the repository's own living docs have no broken links", () => {
  const root = execFileSync("git", ["rev-parse", "--show-toplevel"], {
    encoding: "utf8",
  }).trim();
  const broken = checkRepo(root);
  assert.deepEqual(
    broken,
    [],
    `broken relative links in living docs:\n${broken
      .map((b) => `  ${b.file}:${b.line} -> ${b.target}`)
      .join("\n")}`,
  );
});
