import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { isolatedGit } from "./adr-files.mjs";

test("source checks ignore inherited repository pointers and inspect the requested checkout", (context) => {
  const root = mkdtempSync(join(tmpdir(), "industrial-soak-git-"));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  const candidate = join(root, "candidate");
  const foreign = join(root, "foreign");
  for (const directory of [candidate, foreign]) {
    mkdirSync(directory);
    assert.notEqual(isolatedGit(["init", "--quiet"], directory), null);
    writeFileSync(join(directory, "input.txt"), `${directory}\n`);
    assert.notEqual(isolatedGit(["add", "input.txt"], directory), null);
    assert.notEqual(
      isolatedGit(
        [
          "-c",
          "user.name=Soak Test",
          "-c",
          "user.email=soak@example.invalid",
          "-c",
          "commit.gpgsign=false",
          "commit",
          "--quiet",
          "-m",
          "fixture",
        ],
        directory,
      ),
      null,
    );
  }
  const [commit, tree] = isolatedGit(
    ["rev-parse", "HEAD", "HEAD^{tree}"],
    candidate,
  ).split(/\r?\n/);
  const metadata = isolatedGit(["rev-parse", "--absolute-git-dir"], foreign);
  const environment = {
    ...process.env,
    GIT_DIR: metadata,
    GIT_COMMON_DIR: metadata,
    GIT_WORK_TREE: foreign,
    GIT_INDEX_FILE: join(metadata, "index"),
    GIT_OBJECT_DIRECTORY: join(metadata, "objects"),
    GIT_ALTERNATE_OBJECT_DIRECTORIES: join(metadata, "objects"),
  };
  const sourceModule = new URL("./industrial-soak.mjs", import.meta.url).href;
  const query = `import { readSource } from ${JSON.stringify(sourceModule)}; console.log(JSON.stringify(readSource(process.argv[1])));`;
  const inspect = (directory) =>
    spawnSync(
      process.execPath,
      ["--input-type=module", "-e", query, directory],
      {
        cwd: root,
        env: environment,
        encoding: "utf8",
        timeout: 10_000,
      },
    );
  const assertSource = (clean) => {
    const result = inspect(candidate);
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(JSON.parse(result.stdout), { commit, tree, clean });
  };

  // Inherited Git pointers certified the other clean checkout, hiding real input changes.
  assertSource(true);
  writeFileSync(join(candidate, "input.txt"), "changed\n");
  assertSource(false);
  writeFileSync(join(candidate, "input.txt"), `${candidate}\n`);
  writeFileSync(join(candidate, "untracked.txt"), "new input\n");
  assertSource(false);
  rmSync(join(candidate, "untracked.txt"));
  assertSource(true);

  writeFileSync(join(candidate, ".git", "index"), "invalid index\n");
  const corrupt = inspect(candidate);
  assert.equal(corrupt.status, 1, corrupt.stderr);
  assert.equal(corrupt.stdout, "");
  assert.match(
    corrupt.stderr,
    /cannot read endurance source checkout with Git/,
  );

  const missing = inspect(join(root, "absent"));
  assert.equal(missing.status, 1, missing.stderr);
  assert.equal(missing.stdout, "");
  assert.match(
    missing.stderr,
    /cannot read endurance source checkout with Git/,
  );
  assert.equal(isolatedGit(["status", "--porcelain"], foreign), "");
});
