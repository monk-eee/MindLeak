// Publish the current task branch from any clean checkout or linked worktree.
// The exact HEAD is pushed to the same branch name: this helper never rewrites
// commits, selects another destination, or advances a protected branch.

import { execFileSync } from "node:child_process";

import {
  armedPullRequestNumber,
  armedRefusal,
  queryPullRequest,
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
// So arming means finished. If more work is coming, disarm first.
const armed = armedPullRequestNumber(queryPullRequest(branch, repoRoot));
if (armed !== null) {
  fail(armedRefusal(armed, branch));
}

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

if (server && /^[0-9a-f]{32}$/.test(sessionId)) {
  let changed = [];
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

run(["push", remote, `HEAD:refs/heads/${branch}`], {
  cwd: repoRoot,
  env: { ...process.env, MINDLEAK_CANONICAL_PUBLISH: "1" },
});
console.log(`canonical-push: published HEAD -> ${remote}/${branch}`);
