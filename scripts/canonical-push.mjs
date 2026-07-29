// Publish the current task branch from any clean checkout or linked worktree.
// The exact HEAD is pushed to the same branch name: this helper never rewrites
// commits, selects another destination, or advances a protected branch.

import { execFileSync } from "node:child_process";

import {
  armedPullRequestNumber,
  disarmPullRequest,
  publishPromisedBranch,
  queryPullRequest,
  rearmFailure,
  rearmPullRequest,
} from "./auto-merge-guard.mjs";
import {
  callTools,
  declaredContext,
  identityCollisionNotice,
  overlapNotice,
  publishVerdict,
  resolveServer,
  sameSession,
} from "./claim-gate.mjs";
import { recordPublication } from "./publication-record.mjs";

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

// A release tag is not a branch publication and must not be judged as one.
// Publishing `vX.Y.Z` is the documented release step (DEVELOPERS.md), but the
// branch guard below rejected it three ways at once: `symbolic-ref` fails when
// tagging from a detached HEAD, a tag has no claim to check, and the publisher
// flag is only set when this script pushes a branch. The documented command was
// therefore impossible to run, and the only way to ship a release was an
// undocumented environment variable — folklore, one retirement away from a
// release nobody can cut.
//
// The refs being pushed cannot be read from stdin: pre-commit consumes it
// before the hook runs (verified — the fd is unreadable). It exposes the
// destination as PRE_COMMIT_REMOTE_BRANCH instead.
const pushedRef = process.env.PRE_COMMIT_REMOTE_BRANCH ?? "";
const TAG_PREFIX = "refs/tags/";

/**
 * A tag may only name a commit that has already landed on the protected branch.
 * That is the whole invariant: tagging is how a release is chosen, and choosing
 * an unmerged commit ships code that never passed review.
 */
const verifyTagPublication = (ref) => {
  const tag = ref.slice(TAG_PREFIX.length);

  let commit;
  try {
    commit = git(["rev-list", "-n", "1", ref]);
  } catch {
    return fail(`${tag} does not resolve to a commit`);
  }

  // Judge against the remote's current main, not a possibly stale local copy:
  // a false rejection here blocks a release and sends the next person looking
  // for a bypass, which is the failure this whole change exists to remove.
  try {
    run(["fetch", "--quiet", "origin", "main"], { cwd: repoRoot });
  } catch {
    return fail("cannot reach origin to confirm the tag is on main");
  }

  try {
    execFileSync(
      "git",
      ["merge-base", "--is-ancestor", commit, "origin/main"],
      {
        cwd: repoRoot,
        stdio: "ignore",
      },
    );
  } catch {
    return fail(
      `${tag} names ${commit.slice(0, 7)}, which is not on origin/main; ` +
        "a release tag must name a commit that has landed",
    );
  }

  console.log(
    `canonical-push: ${tag} -> ${commit.slice(0, 7)}, contained in origin/main`,
  );
};

if (verifyPrePush && pushedRef.startsWith(TAG_PREFIX)) {
  verifyTagPublication(pushedRef);
  process.exit(0);
}

let branch;
try {
  branch = git(["symbolic-ref", "--quiet", "--short", "HEAD"]);
} catch {
  fail("HEAD is detached; publish from an attached task branch");
}
if (branch === "main" || branch === "master") {
  fail(
    "direct protected-branch publication is forbidden; use a fleet branch and PR",
  );
}

if (git(["status", "--porcelain", "--untracked-files=normal"])) {
  fail("the worktree has uncommitted changes; finish a scoped commit first");
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

// Arming auto-merge is a promise to merge whatever is on the branch the moment
// checks go green. Pushing after that promise is made is a second writer to the
// same decision, and the branch loses: PR #37 merged at 08:09:21Z and the next
// commit landed 13 seconds later, stranding four commits with nothing reported.
//
// So the promise is withdrawn for the length of the push and re-made about the
// tip that was actually published. Refusing the push held the same invariant,
// but it made every follow-up commit a manual disarm/re-arm dance and pushed
// people toward arming late, which means sitting and watching a pull request
// instead. Nobody merges by hand here, and nobody disarms by hand either.
const armed = armedPullRequestNumber(queryPullRequest(branch, repoRoot));

// Publication requires a live claim (ADR-0049). This is the one place the
// intent plane is not optional: a push is where work becomes visible to the
// rest of the fleet, so it is where the ledger has to already know what the
// work was for. Commits stay ungated - a commit is a draft, a push is a claim
// about the world.
const sessionId = process.env.LODESTAR_SESSION_ID || "";
const server = resolveServer(repoRoot);
let agent = "";
let reachable = false;
let tasks = [];
let overlaps = [];
let declaredBranch = null;
// Shared with the post-push record: the files this push makes visible to the
// fleet are the same ones the overlap notice reasons about, and recomputing
// them afterwards would let the two disagree.
let changed = [];

if (server && /^[0-9a-f]{32}$/.test(sessionId)) {
  try {
    changed = git(["diff", "--name-only", `${remote}/main...HEAD`])
      .split("\n")
      .map((path) => path.trim())
      .filter(Boolean);
  } catch {
    // A branch with no common ancestor still publishes; the overlap notice is
    // advisory and must never be the reason a push fails.
  }
  let behind;
  try {
    behind = Number(
      git(["rev-list", "--count", `HEAD..${remote}/main`]).trim(),
    );
  } catch {
    // Undeclared reports as unknown rather than being guessed (ADR-0044).
  }
  // Declare where this session is working while we are already here. The push
  // is the one moment these facts are certainly true, and the fleet view is
  // only as good as the last thing somebody declared.
  const context = declaredContext({
    branch,
    headSha: git(["rev-parse", "HEAD"]),
    base: `${remote}/main`,
    behind,
  });
  try {
    // `fleet_view` first, deliberately: it must be read *before* this push
    // declares its own context, or the "previously declared branch" would be
    // the declaration made microseconds earlier and the collision check would
    // compare a value with itself.
    const [fleet, session, board, overlapResult] = callTools(server, repoRoot, [
      { name: "fleet_view", arguments: {} },
      {
        name: "open_session",
        arguments: { session_id: sessionId, ...context },
      },
      { name: "board", arguments: { include_terminal: false } },
      { name: "check_overlap", arguments: { paths: changed } },
    ]);
    // Identity is whatever the ledger says this session is, never what the
    // caller asserts: a claim is recorded against the resolved agent id, so
    // matching on anything else would compare two different things.
    agent = session?.agent_id ?? "";
    declaredBranch =
      (fleet?.sessions ?? []).find((entry) =>
        sameSession(entry.agent_id, agent),
      )?.context?.branch ?? null;
    reachable = Boolean(agent) && Array.isArray(board);
    tasks = Array.isArray(board) ? board : [];
    overlaps = overlapResult ?? [];
  } catch {
    reachable = false;
  }
}

const verdict = publishVerdict({
  reachable,
  sessionDeclared: Boolean(sessionId),
  agent,
  tasks,
  branch,
  now: Math.floor(Date.now() / 1000),
});
if (!verdict.ok) {
  fail(verdict.message);
}

const notice = overlapNotice(
  overlaps,
  verdict.claims.map((claim) => claim.id),
  agent,
);
if (notice) {
  console.warn(`canonical-push: ${notice}`);
}

const collision = identityCollisionNotice({
  agent,
  branch,
  declaredBranch,
  claims: verdict.claims,
});
if (collision) {
  console.warn(`canonical-push: ${collision}`);
}

const promise = publishPromisedBranch({
  number: armed,
  disarm: (number) => disarmPullRequest(number, repoRoot),
  push: () =>
    run(["push", remote, `HEAD:refs/heads/${branch}`], {
      cwd: repoRoot,
      env: { ...process.env, MINDLEAK_CANONICAL_PUBLISH: "1" },
    }),
  rearm: (number) => rearmPullRequest(number, repoRoot),
});
console.log(`canonical-push: published HEAD -> ${remote}/${branch}`);
if (promise.cycled && promise.rearmed) {
  console.log(
    `canonical-push: auto-merge re-armed on pull request #${armed}, now promising this commit`,
  );
}
if (promise.cycled && !promise.rearmed) {
  console.warn(`canonical-push: ${rearmFailure(armed)}`);
}

// Recorded after the push, never before: this is evidence that a publication
// happened, and writing it first would assert something that might not.
const unrecorded = recordPublication({
  repoRoot,
  sessionId,
  sha: git(["rev-parse", "HEAD"]),
  message: git(["log", "-1", "--pretty=%B"]),
  changedFiles: changed,
});
if (unrecorded) {
  console.warn(`canonical-push: ${unrecorded}`);
}

// Publication is when the work becomes visible to the fleet, which makes it the
// honest moment to measure what the fleet now has to live with. Reported here
// rather than in CI because the Intent Plane is a per-developer local store: an
// observation recorded on a throwaway runner is recorded nowhere.
//
// Never fatal. The clause behind this control resolves at `review` and the
// control's power is `observed`, so failing the push on a regression would
// enforce harder than the rule it serves (ADR-0034). A rising count is a
// question for a human, not a locked door.
try {
  execFileSync(process.execPath, ["scripts/observe-module-length.mjs"], {
    cwd: repoRoot,
    stdio: "inherit",
    env: process.env,
  });
} catch {
  console.warn(
    "canonical-push: the module-length ratchet was not observed for this publication",
  );
}
