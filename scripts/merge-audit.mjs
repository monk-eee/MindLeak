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
 * The open pull request whose head still carries this commit, or null.
 *
 * A commit pushed onto a branch after its pull request merged is not
 * automatically lost: it is lost only if no open pull request would bring it
 * in. When one would, the work is in review, and the audit's own instruction —
 * open a follow-up pull request — is unactionable, because that is exactly what
 * already exists. An audit that demands something already done is the same
 * failure this file was written to remove, wearing a different costume: it
 * cannot be satisfied, so it gets switched off, taking the check that catches
 * genuinely stranded work with it.
 *
 * Measured on 2026-07-30 with main red on precisely this: commit ffab86ea, held
 * against PR #213's merged branch, is an ancestor of the open PR #231.
 */
export const reviewingRef = (cwd, sha, openRefs) =>
  openRefs.find((ref) => {
    try {
      capture("git", ["merge-base", "--is-ancestor", sha, ref], { cwd });
      return true;
    } catch {
      return false;
    }
  }) ?? null;

/**
 * Split commits that never reached the base into those an open pull request
 * still carries and those nothing carries.
 *
 * `missing` entries are `git cherry -v` lines, so the sha is the first token.
 */
export const splitByReview = (cwd, missing, openRefs) => {
  const stranded = [];
  const inReview = [];
  for (const commit of missing) {
    const sha = commit.split(/\s+/)[0];
    const ref = openRefs.length ? reviewingRef(cwd, sha, openRefs) : null;
    if (ref) inReview.push({ commit, ref });
    else stranded.push(commit);
  }
  return { stranded, inReview };
};

/**
 * Whether a merge commit's tree is the one merging its two parents produces.
 *
 * This is the retrospective form of the guard in `delivery-queue.mjs`, and it
 * closes that guard's stated blind spot. `gh pr update-branch` was measured on
 * 2026-08-18 producing a merge commit that reported clean while its tree kept
 * only one side, silently dropping PR #507's `ListActiveClaims` — no conflict
 * marker, no failed check, and the work simply never reached `main`. The
 * delivery queue now compares trees before it merges, but only at its own call
 * site: a merge made through the web UI, or by anything else, was invisible.
 *
 * A merge commit keeps both parents, so this works long after the branch is
 * deleted — which matters, because branch deletion is what blinds the
 * `git cherry` audit above.
 *
 * Returns `compared: false` rather than a verdict when the merge has no second
 * parent (a squash or rebase merge) or when the parents do not merge cleanly
 * (a human resolved a real conflict, and their tree is *supposed* to differ).
 * Measured over `main`'s last 120 merges: 115 compared and matched, 5 honestly
 * uncomparable, 0 mismatches — so a mismatch is a signal, not noise.
 */
export const mergeIsFaithful = (cwd, mergeCommitSha) => {
  if (!mergeCommitSha) return { compared: false };
  let second;
  try {
    second = capture(
      "git",
      ["rev-parse", "--verify", "--quiet", `${mergeCommitSha}^2^{commit}`],
      { cwd },
    );
  } catch {
    return { compared: false };
  }
  try {
    const first = capture("git", ["rev-parse", `${mergeCommitSha}^1`], { cwd });
    const expected = capture(
      "git",
      ["merge-tree", "--write-tree", first, second],
      { cwd },
    );
    const actual = capture("git", ["rev-parse", `${mergeCommitSha}^{tree}`], {
      cwd,
    });
    return { compared: true, faithful: expected === actual, expected, actual };
  } catch {
    return { compared: false };
  }
};

/**
 * Audit each merged branch against the base.
 *
 * A branch whose tip is already an ancestor of the base is clean by
 * construction, so this reports only what is genuinely missing. Deleted
 * branches are reported as unverifiable rather than clean: "we cannot tell" and
 * "nothing was lost" are different answers, and printing the second when the
 * first is true is how an audit starts lying.
 *
 * `branches` are `{ ref, mergeCommit }`. The `git cherry` half needs the branch
 * to still exist; the `mergeIsFaithful` half does not, which is the only reason
 * a deleted branch is still worth asking about at all.
 */
export const auditBranches = (cwd, baseRef, branches) =>
  branches.map(({ ref, mergeCommit }) => {
    let exists = true;
    try {
      capture("git", ["rev-parse", "--verify", "--quiet", `${ref}^{commit}`], {
        cwd,
      });
    } catch {
      exists = false;
    }
    const merge = mergeIsFaithful(cwd, mergeCommit);
    if (!exists) {
      return {
        branchRef: ref,
        verifiable: false,
        missing: [],
        replaced: [],
        merge,
      };
    }
    return {
      branchRef: ref,
      verifiable: true,
      merge,
      ...classifyCommits(cwd, baseRef, ref),
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
        "number,headRefName,mergeCommit",
      ]),
    );
  } catch {
    console.error(
      "merge-audit: could not list merged pull requests (is `gh` installed and authenticated?)",
    );
    process.exit(2);
  }

  const byBranch = new Map();
  const mergeCommitByBranch = new Map();
  for (const pr of merged) {
    if (!byBranch.has(pr.headRefName)) {
      byBranch.set(pr.headRefName, pr.number);
      mergeCommitByBranch.set(pr.headRefName, pr.mergeCommit?.oid ?? null);
    }
  }

  // The open pull requests, so a commit still on its way in is not mistaken for
  // one that was left behind. Failing to list them degrades to the old, noisier
  // behaviour rather than to a false all-clear: an unreachable `gh` must not be
  // able to silence the audit.
  let openRefs = [];
  const openByRef = new Map();
  try {
    const open = JSON.parse(
      capture(gh, [
        "pr",
        "list",
        "--state",
        "open",
        "--limit",
        limit,
        "--json",
        "number,headRefName",
      ]),
    );
    for (const pr of open) {
      const ref = `origin/${pr.headRefName}`;
      if (!openByRef.has(ref)) openByRef.set(ref, pr.number);
    }
    openRefs = [...openByRef.keys()];
  } catch {
    console.error(
      "merge-audit: could not list open pull requests; every missing commit will be reported as stranded",
    );
  }
  const branches = [...byBranch.keys()];
  const results = auditBranches(
    cwd,
    base,
    branches.map((name) => ({
      ref: `origin/${name}`,
      mergeCommit: mergeCommitByBranch.get(name),
    })),
  );

  let lost = 0;
  let rewritten = 0;
  let waiting = 0;
  let lossy = 0;
  let treeChecked = 0;
  for (const result of results) {
    const name = result.branchRef.replace(/^origin\//, "");
    // Asked of every merged pull request, verifiable branch or not: a merge
    // commit keeps both parents, so this is the half that still works once the
    // branch is deleted.
    if (result.merge.compared) {
      treeChecked += 1;
      if (!result.merge.faithful) {
        lossy += 1;
        console.error(
          `merge-audit: PR #${byBranch.get(name)} (${name}) merged, but its merge commit's tree is not the\n` +
            `    one merging its two parents produces — content was changed or dropped by the merge itself.\n` +
            `    expected tree ${result.merge.expected}, actual tree ${result.merge.actual}`,
        );
      }
    }
    if (!result.verifiable) continue;
    if (result.missing.length) {
      const { stranded, inReview } = splitByReview(
        cwd,
        result.missing,
        openRefs,
      );
      for (const { commit, ref } of inReview) {
        waiting += 1;
        console.log(
          `merge-audit: PR #${byBranch.get(name)} (${name}) has a commit that landed after it merged, ` +
            `now in review on PR #${openByRef.get(ref)}:`,
        );
        console.log(`    ${commit}`);
      }
      if (stranded.length) {
        lost += 1;
        console.error(
          `merge-audit: PR #${byBranch.get(name)} (${name}) merged, but ${stranded.length} commit(s) never reached ${base}:`,
        );
        for (const commit of stranded) console.error(`    ${commit}`);
      }
      if (stranded.length) continue;
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
  if (lossy) {
    // Failed, unlike a rewritten merge, because there is a green move: the
    // dropped hunk can be re-applied in a fresh pull request. This is the one
    // failure mode where every other signal reads healthy — the pull request
    // says merged, the branch still shows the code, and CI was green on both —
    // so nothing else in the system will ever raise it.
    console.error(
      `\nmerge-audit: ${lossy} merge commit(s) do not match the merge of their own parents. Work that the\n` +
        "branch plainly contained may not be on the base. Diff the merge commit against its second parent\n" +
        "and re-apply anything missing in a new pull request; do not trust 'merged with green checks' here.",
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
  if (waiting) {
    // Stated rather than passed over in silence. Pushing onto a branch whose
    // pull request has merged is still a mistake worth seeing — that pull
    // request will never reopen, so the commit depends entirely on the open one
    // that happens to carry it. It is just not lost, and not the build's
    // business to fail on.
    console.log(
      `\nmerge-audit: ${waiting} commit(s) landed on an already-merged branch and are in review elsewhere.\n` +
        "Nothing is lost. Push to the open pull request's branch instead: a merged pull request never\n" +
        "reopens, so a commit pushed there survives only for as long as something else carries it.",
    );
  }
  const verifiable = results.filter((r) => r.verifiable).length;
  const unverifiable = results.length - verifiable;
  console.log(
    `merge-audit: ${verifiable} of ${results.length} merged branch(es) fully landed on ${base}; ` +
      `${treeChecked} of ${results.length} merge commit(s) faithfully merged their own parents`,
  );
  if (unverifiable) {
    // Named and counted, not skipped. Printing only what was checked reads as
    // an all-clear over the whole population, which is the one thing an audit
    // must never imply: the branches it could not check are exactly where lost
    // work would hide. Measured on CI run 99036902566 (2026-08-29), 10 of 30
    // pull requests were dropped this way and the summary mentioned none of
    // them. The tree check above still covers these, because a merge commit
    // keeps both parents after the branch is gone — but `git cherry` cannot,
    // so commits pushed to a branch after it merged and before it was deleted
    // remain genuinely undetectable.
    console.warn(
      `merge-audit: ${unverifiable} merged branch(es) no longer exist, so commits pushed onto them after\n` +
        "they merged cannot be detected. Their merge commits were still tree-checked above. This is not an\n" +
        "all-clear for them; keep branches until the audit has run if that distinction matters:",
    );
    for (const result of results.filter((r) => !r.verifiable)) {
      const name = result.branchRef.replace(/^origin\//, "");
      console.warn(`    PR #${byBranch.get(name)} (${name})`);
    }
  }
}
