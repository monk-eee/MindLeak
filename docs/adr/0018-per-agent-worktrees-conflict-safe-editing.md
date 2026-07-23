# ADR-0018: Per-agent worktrees for conflict-safe concurrent editing

- Status: Proposed
- Date: 2026-07-23
- Deciders: MindLeak maintainers
- Related: [ADR-0004](0004-intent-plane-spec-brain.md) (intent plane),
  [ADR-0015](0015-advisory-symbol-leases.md) (progressive handoffs, no symbol
  lease), [SPEC-INTENT.md](../SPEC-INTENT.md)

## Context

Multiple local agents currently share **one checkout** — one working directory,
one git index, one `.git`. Lodestar coordinates *tasks* (claim/lease CAS,
`blocked_by` handoffs), and [ADR-0015](0015-advisory-symbol-leases.md)
deliberately declined to lock symbols or text because **Lodestar does not own the
filesystem and performs no merges**. So nothing today prevents two claimed tasks
from writing the same working tree at the same time.

### The incident this ADR exists to name (observed 2026-07-23)

While two agents ran concurrently against one checkout, every failure mode below
occurred in a single session:

1. **Clobber / mid-write race.** Two agents began splitting the same module
   (`graph.rs`) into a `graph/` directory simultaneously. One agent's `git mv`
   briefly moved the *other* agent's already-modified file; only a byte-preserving
   round-trip avoided data loss.
2. **Mis-attribution.** The shared index held staged hunks from several agents at
   once; a stray `git add -A` / `commit -a` from any of them would have swept
   unrelated work into the wrong commit under the wrong message. Scoped,
   pathspec-guarded commits were required to stay clean.
3. **Shared-index contention.** `git diff --cached` showed a mix of files owned by
   different agents, so no agent could trust the index state.
4. **Validation poisoning.** A green, ready-to-push commit **could not be pushed**:
   the `pre-push` hook runs `cargo test --all` against the *whole working tree*,
   which included another agent's non-compiling WIP (`store.rs` mid-refactor).
   One agent's broken uncommitted code blocked every other agent's push. It was
   only resolved by pushing from a throwaway `git worktree` checked out at the
   green commit.

ADR-0015 foresaw this and left the door open: *"Add advisory leases only if a
later real-agent scenario shows agents still selecting colliding same-file work."*
This is that scenario — but the evidence points at **filesystem isolation**, not
advisory locking, as the honest fix.

## Options considered

| Option | Guarantee | Cost | Verdict |
|---|---|---|---|
| **(a) Per-agent git worktrees** | **Real** filesystem isolation | disk + per-worktree recompiles | **Primary** |
| (b) Task→path ownership advisories at claim time | Advisory only | claim must pre-declare paths | Secondary aid, not safety |
| (c) Pre-write lease / dirty-path check | Advisory, racy (TOCTOU) | write-interception hook | Rejected as safety |
| (d) Commit-early micro-commit discipline | Shrinks the window | goodwill | Complementary, not sufficient |

- **(a) Per-agent worktrees.** `git worktree add` gives each agent its own working
  directory *and its own index*, sharing one `.git` object store. Isolation is
  real, not advisory: agents in separate directories cannot clobber each other's
  uncommitted files, cannot contend on one index, and cannot sweep each other's
  hunks. Hooks run per-worktree, so one agent's broken WIP cannot block another's
  commit or push (failure mode 4). Integration happens through ordinary git
  (`fetch` + `rebase`/`merge` onto `main`), where genuine overlaps surface as
  **honest text-level merge conflicts** rather than silent loss. Cost: N working
  trees on disk and a compile cache per worktree (mitigable), plus a provisioning
  step. Proven in-session: the blocked push above succeeded from an isolated
  worktree.
- **(b) Path-ownership advisories.** The claim declares the paths it will touch;
  overlapping claims warn or deny. This is the symbol-lease trap ADR-0015 named,
  moved up to file granularity: it *looks* like protection but cannot stop a
  determined or buggy writer, because the plane still does not own the filesystem.
  It also requires agents to know their file set at claim time, which they often
  do not. Useful only as a **planning hint surfaced on the board**, never as the
  safety guarantee.
- **(c) Pre-write dirty-path check.** Check-then-write is racy (TOCTOU) and
  requires intercepting every write path — editors, `cargo`, formatters — which
  agents can bypass. Same false-safety failure as (b).
- **(d) Micro-commit discipline.** Committing early and often shrinks the collision
  window and is good practice, but agents still share one tree and one index, so
  it fixes neither shared-index contention nor validation poisoning on its own.

## Decision

Adopt **(a) per-agent git worktrees** as the primary isolation mechanism,
complemented by **(d) micro-commit discipline**. Do **not** adopt (b) or (c) as
the safety guarantee — for the same reason ADR-0015 rejected symbol locks: an
advisory that looks like a mutex but cannot prevent a text-level stomp grants
**false safety**, which is worse than none.

Shape of the model:

- **One worktree per agent**, provisioned as a sibling of the main checkout (e.g.
  `../MindLeak-agents/<agent-id>`) so it sits **outside** the main tree — the
  passive editor/terminal sensors and file globs must not cross into it. Each
  worktree is a per-agent branch off `origin/main` (or detached at a known base).
- **The main checkout is the human's / integration view.** Agents do not edit it
  directly; they work in their own worktree, commit there, then
  `fetch` + `rebase` onto `origin/main` and push.
- **Compile caches stay per-worktree** by default (true isolation). Sharing one
  `CARGO_TARGET_DIR` across worktrees saves rebuilds but reintroduces a cross-agent
  race on the same build artifacts; if adopted, it must be an explicit, documented
  opt-in, not the default.
- **Lodestar is unchanged.** Claim/lease still coordinates *who* does *what* and
  `blocked_by` still serializes same-goal handoffs; worktrees isolate *where* the
  edits land. No new intent-plane primitive, no filesystem ownership, no merge
  engine — consistent with ADR-0004's loose node-id seam and ADR-0015's refusal to
  become a write-coordinator.

### Rejected alternatives

- **Advisory path ownership / pre-write checks as the safety mechanism** — false
  safety, TOCTOU, and the plane does not own the filesystem (ADR-0015).
- **A real server-side write-coordinator / mini-VCS** — already rejected by
  ADR-0015; turns a small stdio plane into a merge engine. Out of scope.
- **Status quo + micro-commits alone** — leaves validation poisoning and
  shared-index contention unsolved.

## Enforcement and test plan

Proving agents can no longer collide (all platform-agnostic — `git`/`cargo`/
`npm`/`make` only):

1. **Deterministic collision harness.** Provision two worktrees over one object
   store; have each write conflicting edits to the same file and commit. Assert:
   (i) neither clobbers the other's uncommitted file; (ii) each commits
   independently against its own index; (iii) a *non-compiling* WIP in worktree A
   does not fail worktree B's `pre-push` hook; (iv) integrating both onto `main`
   surfaces the overlap as a normal `git merge` conflict — honest, not silent.
2. **Portable provisioning helper.** A cross-platform runner (a Node script under
   `editors/vscode/scripts/` or a `make` target) to add/prune an agent worktree,
   set the per-agent branch, and print the working directory — never a
   PowerShell-/bash-only one-liner.
3. **Docs.** A short runbook section (DEVELOPERS.md) plus a CHANGELOG entry land
   with the tooling (a downstream implementation task, not this ADR).

## Consequences

- **Positive.** Real isolation; per-agent validation (no cross-agent push/commit
  blocking); honest conflict surfacing via plain git; zero new surface in the
  intent plane; reuses a battle-tested primitive.
- **Cost.** N working trees on disk; a compile cache per worktree (mitigable); a
  provisioning/tear-down step; humans must track which worktree is which.
- **Risk.** Agents or humans mis-provision and work in the shared tree anyway
  (mitigate with a `make` target + docs); branch proliferation (mitigate with a
  per-agent naming convention + cleanup on task completion).
- **Invariants preserved.** Workflow-only change: the zero-token deterministic
  hot path, decay, and derived-weight invariants are untouched; no network
  listener is added; everything runs identically on Linux, macOS, and Windows.

This ADR is **design-only**; the provisioning helper and collision harness are a
separate downstream implementation task that unblocks once this decision lands.
