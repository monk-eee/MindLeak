// Merge audit (ADR-0045 clause 4). Reports commits that exist on a branch whose
// pull request is already merged, and therefore never reached the base.
//
// Found in production: PR #37 auto-merged at 08:09:21Z while its branch was
// still being written; the next commit landed 13 seconds later and four commits
// — including the one that stopped two surfaces disagreeing — were left behind.
// Nothing failed. The PR read "merged", the branch read "ahead", CI was green on
// both, and the only signal was an ancestry check nobody was running.
//
// This is the backstop, not the fix. The fix is not arming auto-merge on a
// branch you are still writing (see scripts/canonical-push.mjs); this catches
// the case regardless of how it happened, which is the point of a backstop.
//
// Platform-agnostic: git + node, with `gh` only for enumerating merged pull
// requests. Usage:
//   node scripts/merge-audit.mjs [--base origin/main] [--limit 30]

import { execFileSync } from "node:child_process";

const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const index = args.indexOf(name);
  return index !== -1 && args[index + 1] ? args[index + 1] : fallback;
};

const capture = (command, commandArgs, options = {}) =>
  execFileSync(command, commandArgs, {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    ...options,
  }).trim();

/**
 * Commits on `branchRef` that are not ancestors of `baseRef`, split by whether
 * their work actually reached the base.
 *
 * `git cherry` compares patches rather than commit ids, which is the only way
 * to tell the two apart. Ancestry alone cannot: a squash or rebase merge lands
 * every line and still leaves nothing on the branch reachable from the base, so
 * an ancestry check calls that "work left behind" when the work is right there.
 * That distinction decides whether anyone can act. `missing` is a follow-up
 * pull request waiting to be opened; `replaced` is a fact about history that no
 * commit can undo, because making it an ancestor now would mean rewriting the
 * base.
 *
 * Merge commits are absent from both by construction — `git cherry` skips them,
 * and correctly: merging the base into a branch carries no work of its own, so
 * reporting it as lost work is noise that makes a real report harder to read.
 */
export const classifyCommits = (cwd, baseRef, branchRef) => {
  const log = capture("git", ["cherry", "-v", baseRef, branchRef], { cwd });
  const lines = log ? log.split(/\r?\n/).filter(Boolean) : [];
  const missing = [];
  const replaced = [];
  for (const line of lines) {
    const target = line.startsWith("+") ? missing : replaced;
    target.push(line.slice(2).trim());
  }
  return { missing, replaced };
};

/**
 * Audit each merged branch against the base.
 *
 * A branch whose tip is already an ancestor of the base is clean by
 * construction, so this reports only what is genuinely missing. Deleted
 * branches are reported as unverifiable rather than clean: "we cannot tell" and
 * "nothing was lost" are different answers, and printing the second when the
 * first is true is how an audit starts lying.
 */
export const auditBranches = (cwd, baseRef, branchRefs) =>
  branchRefs.map((branchRef) => {
    let exists = true;
    try {
      capture(
        "git",
        ["rev-parse", "--verify", "--quiet", `${branchRef}^{commit}`],
        {
          cwd,
        },
      );
    } catch {
      exists = false;
    }
    if (!exists) {
      return { branchRef, verifiable: false, missing: [], replaced: [] };
    }
    return {
      branchRef,
      verifiable: true,
      ...classifyCommits(cwd, baseRef, branchRef),
    };
  });

// Importing this module must not run the audit: the tests exercise the pure
// functions above against fixture repositories, with no `gh` and no network.
const invokedDirectly = process.argv[1]?.endsWith("merge-audit.mjs") ?? false;
if (invokedDirectly) {
  const base = opt("--base", "origin/main");
  const limit = opt("--limit", "30");
  const gh = process.env.MINDLEAK_GH_BIN || "gh";
  const cwd = capture("git", ["rev-parse", "--show-toplevel"]);

  capture("git", ["fetch", "--prune", "--quiet", "origin"], { cwd });

  let merged;
  try {
    merged = JSON.parse(
      capture(gh, [
        "pr",
        "list",
        "--state",
        "merged",
        "--limit",
        limit,
        "--json",
        "number,headRefName",
      ]),
    );
  } catch {
    console.error(
      "merge-audit: could not list merged pull requests (is `gh` installed and authenticated?)",
    );
    process.exit(2);
  }

  const byBranch = new Map();
  for (const pr of merged) {
    if (!byBranch.has(pr.headRefName)) byBranch.set(pr.headRefName, pr.number);
  }
  const branches = [...byBranch.keys()];
  const results = auditBranches(
    cwd,
    base,
    branches.map((name) => `origin/${name}`),
  );

  let lost = 0;
  let rewritten = 0;
  for (const result of results) {
    const name = result.branchRef.replace(/^origin\//, "");
    if (!result.verifiable) continue;
    if (result.missing.length) {
      lost += 1;
      console.error(
        `merge-audit: PR #${byBranch.get(name)} (${name}) merged, but ${result.missing.length} commit(s) never reached ${base}:`,
      );
      for (const commit of result.missing) console.error(`    ${commit}`);
      continue;
    }
    if (result.replaced.length) {
      rewritten += 1;
      console.warn(
        `merge-audit: PR #${byBranch.get(name)} (${name}) landed every line, but as ${result.replaced.length} rewritten commit(s) — a squash or rebase merge:`,
      );
      for (const commit of result.replaced) console.warn(`    ${commit}`);
    }
  }

  if (lost) {
    console.error(
      `\nmerge-audit: ${lost} merged branch(es) left work behind. Open a follow-up pull request for each;\n` +
        "do not push the missing commits onto the merged branch, because its pull request will never reopen.",
    );
    process.exit(1);
  }
  if (rewritten) {
    // Reported, not failed. The commit identities are gone and no commit can
    // bring them back, so failing here would mean a red build with no green
    // move available — and an audit that cannot be satisfied gets switched off,
    // taking the check that catches genuinely lost work with it. Prevention
    // belongs where the merge button is: turn off squash and rebase merging on
    // the repository, so the rule stops depending on which button was clicked.
    console.warn(
      `\nmerge-audit: ${rewritten} merged branch(es) landed as rewritten commits. No work was lost, and\n` +
        "nothing can be done about it now. AGENTS.md asks for merge commits so a commit id stays\n" +
        "evidence; disable squash and rebase merging on the repository to enforce it at the button.",
    );
  }
  console.log(
    `merge-audit: ${results.filter((r) => r.verifiable).length} merged branch(es) fully landed on ${base}`,
  );
}
