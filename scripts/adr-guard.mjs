// An ADR is a decision record. Losing one loses the reasoning behind the code,
// and the loss is silent — nothing fails, the file simply is not there any more.
// Three separate near-misses in one session motivated this guard: an ADR staged
// but never committed, an ADR committed only on a branch that was never pushed,
// and an ADR that was never added to Git at all.
//
// Two failure modes, both checked here:
//   1. UNCOMMITTED — an ADR that is untracked or has uncommitted edits. It lives
//      only in the working tree; `git clean -fd` or `git checkout .` erases it.
//   2. UNPUBLISHED — an ADR committed only on local branches, reachable from no
//      remote ref. It survives an editor crash but not a lost disk, a deleted
//      branch, or `git worktree remove`.
//
// Scans every attached worktree and every local branch, because under ADR-0038
// concurrent work is spread across worktrees on different branches and an ADR
// can be stranded in any of them.
//
// Platform-agnostic: git + node only. Usage:
//   node scripts/adr-guard.mjs                  # full audit, exit 1 on findings
//   node scripts/adr-guard.mjs --uncommitted    # working-tree check only
//   node scripts/adr-guard.mjs --format json

import { execFileSync } from "node:child_process";

const args = process.argv.slice(2);
const uncommittedOnly = args.includes("--uncommitted");
const asJson =
  args.includes("--format") && args[args.indexOf("--format") + 1] === "json";

// A child git process must never inherit the parent's repository pointers, or a
// hook-invoked run resolves the wrong repository entirely.
const GIT_REPOSITORY_VARIABLES = [
  "GIT_DIR",
  "GIT_WORK_TREE",
  "GIT_COMMON_DIR",
  "GIT_INDEX_FILE",
  "GIT_OBJECT_DIRECTORY",
  "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

const gitEnvironment = () => {
  const isolated = { ...process.env };
  for (const variable of GIT_REPOSITORY_VARIABLES) delete isolated[variable];
  return isolated;
};

const git = (gitArgs, cwd = process.cwd()) => {
  try {
    return execFileSync("git", gitArgs, {
      cwd,
      encoding: "utf8",
      stdio: "pipe",
      env: gitEnvironment(),
    }).trim();
  } catch {
    return null;
  }
};

const ADR_PATH = /^docs\/adr\/\d{4}-.*\.md$/;
const isAdr = (file) => ADR_PATH.test(file.replace(/\\/g, "/"));

/** Worktrees of this repository, primary first. */
export function worktreePaths(root = process.cwd()) {
  const listing = git(["worktree", "list", "--porcelain"], root);
  if (!listing) return [root];
  return listing
    .split(/\r?\n/)
    .filter((line) => line.startsWith("worktree "))
    .map((line) => line.slice("worktree ".length).trim());
}

/**
 * ADRs present in a worktree but not safely committed there. `git status`
 * porcelain marks untracked with `??`; anything else with a status code has
 * staged or unstaged modifications that no commit holds yet.
 */
export function uncommittedAdrs(worktree) {
  const status = git(
    ["status", "--porcelain", "--untracked-files=all", "--", "docs/adr"],
    worktree,
  );
  if (!status) return [];
  return status
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => ({
      code: line.slice(0, 2).trim(),
      file: line.slice(3).trim().replace(/^"|"$/g, ""),
    }))
    .filter(({ file }) => isAdr(file))
    .map(({ code, file }) => ({
      worktree,
      adrPath: file,
      reason:
        code === "??"
          ? "untracked - not in Git at all"
          : `uncommitted changes (${code})`,
    }));
}

const adrsInRef = (ref, root) => {
  const listing = git(
    ["ls-tree", "-r", "--name-only", ref, "--", "docs/adr"],
    root,
  );
  return listing ? listing.split(/\r?\n/).filter(Boolean).filter(isAdr) : [];
};

const refNames = (pattern, root) => {
  const listing = git(
    ["for-each-ref", "--format=%(refname:short)", pattern],
    root,
  );
  return listing ? listing.split(/\r?\n/).filter(Boolean) : [];
};

/**
 * ADRs reachable from local branches but from no remote ref. Only a remote copy
 * survives the loss of this machine.
 */
export function unpublishedAdrs(root = process.cwd()) {
  const published = new Set();
  for (const ref of refNames("refs/remotes", root)) {
    for (const file of adrsInRef(ref, root)) published.add(file);
  }
  const stranded = new Map();
  for (const ref of refNames("refs/heads", root)) {
    for (const file of adrsInRef(ref, root)) {
      if (published.has(file)) continue;
      if (!stranded.has(file)) stranded.set(file, []);
      stranded.get(file).push(ref);
    }
  }
  return [...stranded]
    .map(([adrPath, branches]) => ({ adrPath, branches: branches.sort() }))
    .sort((left, right) => left.adrPath.localeCompare(right.adrPath));
}

export function auditAdrs(
  root = process.cwd(),
  { uncommitted = true, unpublished = true } = {},
) {
  const uncommittedFindings = uncommitted
    ? worktreePaths(root).flatMap((worktree) => uncommittedAdrs(worktree))
    : [];
  const unpublishedFindings = unpublished ? unpublishedAdrs(root) : [];
  return {
    uncommitted: uncommittedFindings,
    unpublished: unpublishedFindings,
    ok: uncommittedFindings.length === 0 && unpublishedFindings.length === 0,
  };
}

function main() {
  const root = git(["rev-parse", "--show-toplevel"]) ?? process.cwd();
  const report = auditAdrs(root, {
    uncommitted: true,
    unpublished: !uncommittedOnly,
  });

  if (asJson) {
    console.log(JSON.stringify(report, null, 2));
    process.exit(report.ok ? 0 : 1);
  }

  if (report.ok) {
    console.log(
      "adr-guard: every ADR is committed and reachable from a remote ref",
    );
    process.exit(0);
  }

  for (const finding of report.uncommitted) {
    console.error(
      `adr-guard: UNCOMMITTED ${finding.adrPath} (${finding.reason})`,
    );
    console.error(`  in ${finding.worktree}`);
  }
  for (const finding of report.unpublished) {
    console.error(`adr-guard: UNPUBLISHED ${finding.adrPath}`);
    console.error(
      `  only on: ${finding.branches.join(", ")} - push the branch`,
    );
  }
  console.error(
    "\nadr-guard: an ADR that exists in only one place is one accident from gone.",
  );
  process.exit(1);
}

// Only audit when run directly, so the helpers stay importable by tests.
if (
  process.argv[1] &&
  import.meta.url ===
    new URL(`file://${process.argv[1].replace(/\\/g, "/")}`).href
) {
  main();
}
