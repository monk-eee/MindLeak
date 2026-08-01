---
name: mindleak
description: >
  Operate correctly in a repository instrumented with MindLeak (Memory Plane) and
  Lodestar (Intent Plane). Use when the agent needs to: (1) pick up, claim or complete
  work on the task board, (2) publish a branch through canonical-push, (3) diagnose a
  push that reports the work "will not certify", (4) decide whether to take over a
  lapsed claim, (5) work in a git worktree alongside other agents, (6) resolve a refused
  claim, evidence bundle or conformance check, or (7) mentions "the board", "claim",
  "lease", "completion offer", "check_overlap", "evidence", or "canonical-push".
---

# Operating a MindLeak-instrumented repository

Two planes. **MindLeak** is the Memory Plane: a decaying graph of what happened.
**Lodestar** is the Intent Plane: durable goals, claims and conformance. Work is
claimed on one and evidenced by the other, and the two only meet at publication.

For the mental model read [USAGE.md](../../docs/USAGE.md); for worked scenarios
[WALKTHROUGH.md](../../docs/WALKTHROUGH.md); for the tool list
[TOOLS.md](../../docs/TOOLS.md). **This skill is none of those.** It is the
operational discipline those documents assume you already have — the sequence
that makes evidence actually close, and the failure modes that silently prevent
it.

## The loop, in order

1. `open_session` on **both** planes with one client-minted 128-hex id. Reuse it
   on every identity-bearing call, and re-register after any server restart.
2. `task_query view=board` — read before creating. The fleet is fast; work you
   create may be claimed by someone else within seconds.
3. `task_claim step=claim` **with `paths` declared**. Undeclared scope cannot be
   tied to a commit later.
4. `check_overlap` before the first edit, on the concrete paths.
5. Work in an isolated worktree. **Commit early.**
6. `scoped-commit` with named paths. Never `git add -A`.
7. `canonical-push`. With both planes configured it records the commit and
   writes a completion offer.
8. Submit the offer with `task_transition to="complete"`.
9. Reclaim the worktree once **your** pull request has merged.

## Environment: both planes, or evidence degrades silently

```bash
export LODESTAR_SESSION_ID=<32-hex id registered with open_session>
export LODESTAR_MCP_BIN="$HOME/.mindleak/bin/lodestar-mcp"
export MINDLEAK_MCP_BIN="$HOME/.mindleak/bin/mindleak-mcp"
```

Both binaries default to `<repoRoot>/target/{release,debug}`. **A linked worktree
has no `target/` of its own**, so in a fresh worktree neither resolves unless the
overrides are set. Missing `MINDLEAK_MCP_BIN` is the single most expensive
misconfiguration here: the push succeeds, the commit is never recorded, and the
work cannot certify.

Do not point either override at the repository's own `target/release`. That
binary is usually older than the ledger and fails with *"board could not be
read"* — which is a stale binary, not an unreachable one.

## Claims and leases

- **Declare `paths` on every claim.** `merge_evidence` ties a commit to a task
  through declared scope; with an empty scope it refuses.
- **Read the scope before a rescue claim** (`task_query view=scope`). Re-claiming
  is how ownership moves, and the original scope is not recoverable from the task
  afterwards if it is lost.
- **A lease is a heartbeat, not a deadline.** Renew between steps — after a
  build, between files, before a long test run. A lapse frees the task for
  someone else mid-flight.
- **A lost claim is the system working.** `won: false` means another agent got
  there first. Do not `recover` a live claim; find other work.

## Worktrees

- One isolated worktree and branch per workstream:
  `git worktree add -b <branch> <path> origin/main` in **one** command.
- **Commit early.** The ownership marker is written on the *first commit*, and
  guards key on it. Before that, a worktree is indistinguishable from residue.
- A fresh worktree has no extension `node_modules`; install them or the
  formatting hook fails on an unrelated file.
- `"not a git repository"` from inside a directory full of your own files means
  the worktree was unregistered, not that you are lost.
- **Reclaim only what you finished.** Peers who just started look exactly like
  abandoned work.

## Publishing and evidence

`canonical-push` refuses rather than waving through: it checks both planes
*before* pushing, because a warning after an irreversible act is not protection.

On success it records the commit and writes
`target/completion-offers/task-<id>.json` containing the exact `evidence` and
`check` objects. Submit them **by reference** — read the file and pass the
objects through. Never retype a bundle: the conformance token is bound to the
exact bundle it saw, and a hand-copied one is refused.

Preparation is automated; **attestation is not** (ADR-0065). The tooling may
assemble the evidence, but only the agent may declare the work complete.

## Rescuing someone else's work

A lapsed lease renders identically on the board whether the work is abandoned or
merely paused. Three commands separate them, run against the owner's worktree:

```bash
git status --porcelain                        # uncommitted work?
git rev-list --count origin/main..HEAD        # unlanded commits?
gh pr list --head <branch> --state all        # did it merge?
```

| What you find | Do |
|---|---|
| Uncommitted work, no commits | **Leave it.** You cannot commit in a worktree you do not own, so taking it means redoing work that already exists — and if the owner returns there are two versions. |
| Clean, nothing unlanded, PR merged | **Take it.** The work shipped and the claim is residue. Close it with `merge_evidence`, naming the merge commit. |
| Lapsed seconds ago | **Leave it.** That is a live agent between heartbeats, not an abandoned claim. |

Use `merge_evidence`, not `evidence_for`, for work you did not do. `evidence_for`
reads *your* attributed executions and commits, which for a rescue is honestly
empty and certifies nothing.

## When something is refused

| Symptom | Cause | Fix |
|---|---|---|
| `will not certify` after a successful push | `MINDLEAK_MCP_BIN` unset; no binary in a fresh worktree | Set the override, then `ingest_commit` the published commit |
| `board could not be read` | Deployed binary older than the ledger | Point at the shared install, not `target/release` |
| `unknown session_id` | Server restarted | `open_session` again with the same id |
| `task declared no path scope` | Claimed without `paths` | Re-claim with the scope; read it first if rescuing |
| `does not match the live task claim and evidence` | Bundle was retyped or trimmed | Pass `evidence`/`check` by reference from the offer |
| `evidence interval falls outside the live claim` | Window end in the future or outside the claim | Bound the window by the claim and current time |
| Verdict `drift`, "governed code changed without a covering task" | Touched code governed by another goal | Declare `also_serves` **at claim time** — it is refused once conformance has judged |

## What this skill is not

It is not a description of the tools (see [TOOLS.md](../../docs/TOOLS.md)) and
not a contribution guide for MindLeak itself (see
[AGENTS.md](../../AGENTS.md)). It assumes the servers are already installed —
[QUICKSTART.md](../../docs/QUICKSTART.md) covers that.
