// Publish the current fleet branch from the repository's primary checkout.
// The exact HEAD is pushed: this helper never creates side lineages, rewrites
// commits, or targets a different branch.

import { execFileSync } from "node:child_process";
import { resolve } from "node:path";

const args = process.argv.slice(2);
const verifyPrePush = args.includes("--verify-pre-push");
const opt = (name, fallback) => {
  const index = args.indexOf(name);
  return index !== -1 && args[index + 1] ? args[index + 1] : fallback;
};
const fail = (message) => {
  console.error(`canonical-push: ${message}`);
  process.exit(2);
};
const capture = (gitArgs, options = {}) =>
  execFileSync("git", gitArgs, { encoding: "utf8", ...options }).trim();
const run = (gitArgs, options = {}) =>
  execFileSync("git", gitArgs, { stdio: "inherit", ...options });

const repoRoot = capture(["rev-parse", "--show-toplevel"]);
const git = (gitArgs, options = {}) =>
  capture(gitArgs, { cwd: repoRoot, ...options });
const gitDir = resolve(repoRoot, git(["rev-parse", "--git-dir"]));
const commonDir = resolve(repoRoot, git(["rev-parse", "--git-common-dir"]));

if (gitDir.toLowerCase() !== commonDir.toLowerCase()) {
  fail("run from the primary checkout, not a linked worktree");
}

const branch = git(["symbolic-ref", "--quiet", "--short", "HEAD"]);
if (branch === "main" || branch === "master") {
  fail(
    "direct protected-branch publication is forbidden; use a fleet branch and PR",
  );
}

try {
  git(["diff", "--cached", "--quiet"]);
} catch {
  fail(
    "the shared index contains staged changes; finish a scoped commit first",
  );
}

if (verifyPrePush) {
  if (process.env.MINDLEAK_CANONICAL_PUBLISH !== "1") {
    fail("pushes must run through scripts/canonical-push.mjs");
  }
  console.log("canonical-push: pre-push checks passed");
  process.exit(0);
}

const remote = opt("--remote", "origin");
run(["fetch", "--quiet", remote], { cwd: repoRoot });

const remoteRef = `refs/remotes/${remote}/${branch}`;
let remoteBranchExists = true;
try {
  git(["show-ref", "--verify", "--quiet", remoteRef]);
} catch {
  remoteBranchExists = false;
}

if (remoteBranchExists) {
  try {
    git(["merge-base", "--is-ancestor", remoteRef, "HEAD"]);
  } catch {
    fail(
      `${remote}/${branch} is not an ancestor of HEAD; reconcile in this checkout before publishing`,
    );
  }
}

run(["push", remote, `HEAD:refs/heads/${branch}`], {
  cwd: repoRoot,
  env: { ...process.env, MINDLEAK_CANONICAL_PUBLISH: "1" },
});
console.log(`canonical-push: published HEAD -> ${remote}/${branch}`);
