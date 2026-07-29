import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { isFragmentName, readFragments, render } from "./gaps.mjs";

/** A throwaway gaps.d, so no test can see the repository's real gaps. */
const withDir = (files, run) => {
  const dir = mkdtempSync(join(tmpdir(), "gaps-"));
  try {
    mkdirSync(dir, { recursive: true });
    for (const [name, body] of Object.entries(files)) {
      writeFileSync(join(dir, name), body, "utf8");
    }
    run(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
};

const GAP = "- **Something is wrong — OPEN.** It happens in `foo.rs`.";

test("a fragment name is a lowercase kebab slug", () => {
  assert.equal(isFragmentName("a-lapsed-claim-cannot-certify.md"), true);
  assert.equal(isFragmentName("gap1.md"), true);
  assert.equal(isFragmentName("A-Gap.md"), false, "no capitals");
  assert.equal(isFragmentName("a_gap.md"), false, "no underscores");
  assert.equal(isFragmentName("a--gap.md"), false, "no doubled separators");
  assert.equal(isFragmentName("-gap.md"), false, "no leading separator");
});

test("every fragment is read, sorted, and README is not one of them", () => {
  withDir(
    {
      "second.md": `${GAP}\n`,
      "first.md": `${GAP}\n`,
      "README.md": "how to write a gap\n",
    },
    (dir) => {
      const { gaps, problems } = readFragments(dir);
      assert.deepEqual(problems, []);
      assert.deepEqual(
        gaps.map((gap) => gap.name),
        ["first.md", "second.md"],
        "sorted by name so the rendered order is stable",
      );
    },
  );
});

test("a fragment that is not a bullet is reported, not silently skipped", () => {
  // Silently skipping would leave a gap that looks filed and reads as nothing —
  // worse than refusing it, because nobody goes looking for a gap they filed.
  withDir({ "prose.md": "I noticed something odd today.\n" }, (dir) => {
    const { gaps, problems } = readFragments(dir);
    assert.equal(gaps.length, 0);
    assert.match(problems[0], /must open with a "- \*\*" bullet/);
  });
});

test("a misnamed fragment is reported by name", () => {
  withDir({ "Not_A_Slug.md": `${GAP}\n` }, (dir) => {
    const { problems } = readFragments(dir);
    assert.match(problems[0], /^Not_A_Slug\.md: name must be/);
  });
});

test("a missing directory yields nothing rather than throwing", () => {
  const { gaps, files, problems } = readFragments(
    join(tmpdir(), "gaps-does-not-exist-a9f3"),
  );
  assert.deepEqual([gaps, files, problems], [[], [], []]);
});

test("rendering separates gaps by a blank line and adds nothing else", () => {
  withDir({ "a.md": `${GAP}\n`, "b.md": `${GAP}\n` }, (dir) => {
    const { gaps } = readFragments(dir);
    assert.equal(render(gaps), `${GAP}\n\n${GAP}`);
  });
});

test("trailing whitespace is trimmed so joined output cannot drift", () => {
  withDir({ "a.md": `${GAP}\n\n\n` }, (dir) => {
    const { gaps } = readFragments(dir);
    assert.equal(gaps[0].body, GAP);
  });
});
