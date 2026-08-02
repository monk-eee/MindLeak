# Troubleshooting

Failure modes measured in real multi-agent use, with the fix for each. Setup
problems belong in [setup](./setup.md); this file covers what goes wrong *after*
both planes are connected and work is under way.

Most entries here share one shape: **the operation appears to succeed and the
evidence quietly does not exist.** That is why they are worth naming — nothing
fails loudly enough to send you looking.

## Both plane binaries, or evidence degrades silently

Publication tooling resolves each server from an environment override, falling
back to `<repoRoot>/target/{release,debug}`. **A linked worktree has no `target/`
of its own**, so in a fresh worktree neither binary resolves unless the overrides
are set:

```bash
export LODESTAR_SESSION_ID=<the id registered with open_session>
export LODESTAR_MCP_BIN="$HOME/.mindleak/bin/lodestar-mcp"
export MINDLEAK_MCP_BIN="$HOME/.mindleak/bin/mindleak-mcp"
```

Omitting `MINDLEAK_MCP_BIN` is the most expensive misconfiguration in this list:
the push succeeds, the commit is never recorded in the Memory Plane, and the work
cannot certify later. Nothing reports a problem at the time.

Do not point either override at the repository's own `target/release`. That
binary is usually older than the ledger, and the resulting *"board could not be
read"* means a stale binary, not an unreachable server.

## Claims and leases

- **Declare `paths` on every claim.** A commit is tied to a task through declared
  scope; with an empty scope, evidence merging refuses.
- **Read the scope before taking over a claim.** Re-claiming is how ownership
  moves, and the original scope is not recoverable from the task afterwards if a
  re-claim drops it.
- **A lease is a heartbeat, not a deadline.** Renew between steps — after a build,
  between files, before a long test run. A lapse frees the task for another agent
  while you are still working in it.
- **A refused claim is the system working.** Losing the race means another agent
  got there first; find other work rather than recovering a live claim.

## Worktrees

- **Commit early.** The ownership marker is written on the *first commit*, and
  the guards key on it. Before that, a worktree is indistinguishable from
  abandoned residue.
- A fresh worktree has no editor-extension `node_modules`, so a formatting hook
  can fail on a file you never touched. Install them first.
- `"not a git repository"` from inside a directory full of your own files means
  the worktree was unregistered, not that you are lost.
- **Reclaim only what you finished.** A peer who has just started looks exactly
  like abandoned work.

## Rescuing another agent's work

A lapsed lease renders identically whether the work was abandoned or the owner is
simply between heartbeats. Three commands against the owner's worktree separate
them:

```bash
git status --porcelain                        # uncommitted work?
git rev-list --count origin/main..HEAD        # unlanded commits?
gh pr list --head <branch> --state all        # did it merge?
```

| What you find | Do |
|---|---|
| Uncommitted work, no commits | **Leave it.** Taking it means redoing work that already exists, and if the owner returns there are two versions. |
| Clean, nothing unlanded, pull request merged | **Take it.** The work shipped and the claim is residue; close it against the merge commit. |
| Lapsed seconds ago | **Leave it.** That is a live agent between heartbeats. |

For work you did not do, close the task against the merged commit rather than
with your own attributed evidence — the latter is honestly empty and certifies
nothing.

## When something is refused

| Symptom | Cause | Fix |
|---|---|---|
| `will not certify` after a successful push | `MINDLEAK_MCP_BIN` unset; no binary in a fresh worktree | Set the override, then ingest the published commit |
| `board could not be read` | Deployed binary older than the ledger | Point at the shared install, not `target/release` |
| `unknown session_id` | Server restarted since `open_session` | Open the session again with the same id |
| `task declared no path scope` | Claimed without `paths` | Re-claim with the scope; read the existing scope first if taking over |
| Bundle `does not match the live task claim` | Evidence was retyped or trimmed | Pass `evidence`/`check` **by reference** from the completion offer file |
| `evidence interval falls outside the live claim` | Window ends in the future, or outside the claim | Bound the window by the claim and the current time |
| Verdict `drift`, governed code changed without a covering task | Touched code governed by another goal | Declare the additional goal **at claim time** — it is refused once conformance has judged |

Preparation is automated; **attestation is not**. Tooling may assemble an
evidence bundle, but only the agent may declare the work complete — which is why
a hand-copied bundle is rejected: the conformance token is bound to the exact
bundle the server saw.
