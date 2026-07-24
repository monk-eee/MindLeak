# ADR-0018: Conflict-safe concurrent editing in a shared working tree (worktrees optional)

- Status: Accepted
- Date: 2026-07-23
- Deciders: MindLeak maintainers
- Related: [ADR-0004](0004-intent-plane-spec-brain.md) (intent plane),
  [ADR-0015](0015-advisory-symbol-leases.md) (progressive handoffs, no symbol
  lease), [SPEC-INTENT.md](../SPEC-INTENT.md)

## Context

MindLeak and Lodestar exist **precisely to make multiple agents sharing one
working tree safe and productive**. Per-agent git worktrees would make the
collision problem disappear by isolating each agent — but that sidesteps the very
problem this project is built to solve. The maintainer's deliberate workflow is
**one shared checkout**, and the whole point of the intent plane is to coordinate
concurrent work *in that shared tree*. "Just use worktrees" is a valid answer in
general; it is the wrong default here.

So the design goal is not "isolate agents." It is: **what can we honestly do to
make concurrent editing of one shared tree safe, while keeping worktrees available
for anyone who does want isolation** — and crucially, making every mechanism work
identically in both modes.

### The incident this ADR exists to name (observed 2026-07-23)

Two agents running concurrently against one checkout hit every failure mode in a
single session:

1. **Clobber / mid-write race.** Both began splitting the same module (`graph.rs`)
   into a `graph/` directory at once; one agent's `git mv` briefly moved the
   *other* agent's already-modified file. Only a byte-preserving round-trip
   avoided data loss.
2. **Mis-attribution.** The shared index held staged hunks from several agents; a
   stray `git add -A` / `commit -a` would have swept unrelated work into the wrong
   commit under the wrong message. Scoped, pathspec-guarded commits were required.
3. **Shared-index contention.** `git diff --cached` mixed files owned by different
   agents, so no agent could trust the index.
4. **Validation poisoning.** A green, ready-to-push commit **could not be pushed**:
   the `pre-push` hook runs `cargo test --all` against the *whole working tree*,
   which included another agent's non-compiling WIP. One agent's broken uncommitted
   code blocked every other agent's push — resolved only by pushing from a
   throwaway `git worktree` at the green commit.

[ADR-0015](0015-advisory-symbol-leases.md) already established the honesty rule:
Lodestar does not own the filesystem and performs no merges, so an advisory that
looks like a mutex but cannot stop a text-level stomp grants **false safety**.

## Options considered

| Option | Guarantee | Fit for this project | Verdict |
|---|---|---|---|
| (a) Mandate per-agent worktrees | Real isolation | Sidesteps the problem the project exists to solve; maintainer opts out | Available, **not** mandated |
| **(b) Coordinated shared tree: claim + commit discipline + isolated validate/push** | Honest, tooling-enforced | Matches the shared-tree workflow directly | **Primary** |
| (c) Advisory path / dirty-path locking as a guarantee | Advisory only (false safety) | Same trap as ADR-0015 symbol locks | Board hint only |
| (d) Server-side write-coordinator / mini-VCS | Real, but huge | Turns the stdio plane into a merge engine | Rejected (ADR-0015) |

## Decision

Support **both modes as a user choice**, and do **not** mandate worktrees. Invest
in making the **shared single tree** safe — the workflow the project is built for —
while keeping worktrees a fully-supported opt-in escape hatch.

### Primary: coordinate the shared tree (default, first-class)

1. **Lodestar claim/lease + `blocked_by` handoffs** serialize same-goal / same-file
   work (existing mechanism; ADR-0015). This stays the coordination backbone.
2. **Commit discipline, enforced by portable tooling.** Scoped, pathspec-guarded
   commits that stage only the agent's own paths; **never** `git add -A` /
   `commit -a`; micro-commit early to shrink the shared-index window. A guard
   aborts the commit if the staged set escapes the agent's declared paths (the
   exact technique that kept commits clean during the incident above).
3. **Isolated validate/push as a tactic, not a workflow.** When other agents'
   uncommitted WIP would poison a pre-commit/pre-push hook, validate and push the
   agent's *green* commit from a throwaway `git worktree` checked out at that
   commit, then discard it. This borrows worktrees as a momentary tool without
   adopting them as the working model — proven in-session.
4. **Optional advisory path-ownership as a board hint only.** A claim may declare
   the paths it intends to touch so the board can *warn* on overlap. Never a
   guarantee, never a block (ADR-0015 false-safety rule).

### Opt-in: per-agent worktrees (escape hatch)

Per-agent worktrees remain fully supported and documented for anyone who wants
true filesystem isolation. Provisioned as a sibling of the main checkout (outside
it, so passive sensors do not cross), a per-agent branch off `origin/main`.
Available, not the default.

### The load-bearing caveat: mode-agnostic tooling

Every tool and convention we build — the commit-scope guard, the isolated
validate/push helper, the optional path-hint — **must work identically whether the
agent runs in the shared tree or in a worktree**. No tool may assume isolation, and
none may assume a shared tree. This is what lets the maintainer opt out of
worktrees while the mechanisms still serve someone who opts in.

### Rejected alternatives

- **Mandating worktrees** — sidesteps the problem MindLeak/Lodestar exist to solve
  and contradicts the maintainer's deliberate shared-tree workflow. Kept as an
  option, never the plan.
- **Advisory path / dirty-path locking as a hard guarantee** — false safety,
  TOCTOU, filesystem not owned by the plane (ADR-0015).
- **Server-side write-coordinator / mini-VCS** — already rejected by ADR-0015.

## Enforcement and test plan

Platform-agnostic (`git`/`cargo`/`npm`/`make` only) **and** mode-agnostic:

1. **Commit-scope guard.** A portable helper (a `make` target / Node script) that
   stages only declared paths and aborts if the staged set escapes them. Test: a
   shared tree dirty with other agents' files → the guard commits only the agent's
   paths and refuses otherwise.
2. **Isolated validate/push helper.** A portable command that creates a throwaway
   worktree at a given commit, runs the hooks/tests there, pushes, and cleans up —
   so one agent's broken shared-tree WIP cannot block another's push. Test: broken
   WIP present in the shared tree; the helper still validates and pushes the green
   commit.
3. **Mode-agnostic assertion.** Both helpers must run unchanged from inside a
   per-agent worktree as well as the shared tree.
4. **Optional path-hint.** The board surfaces overlapping declared paths; test that
   it warns and never blocks.
5. **Docs.** A DEVELOPERS.md runbook covering both modes, plus a CHANGELOG entry,
   land with the tooling (a downstream implementation task, not this ADR).

## Consequences

- **Positive.** The shared-tree workflow the project is built for becomes safe
  without forcing isolation; worktrees remain available for those who want them;
  the tooling is mode-agnostic; it reuses git + Lodestar with no new intent-plane
  surface and no network listener.
- **Cost.** Agents must adopt commit discipline and use the guard / validate
  helpers; the isolated-push tactic spins up a transient worktree per push.
- **Risk.** Agents may ignore the discipline — mitigated by making the safe path
  the easy path (tooling) and surfacing overlap on the board; the shared tree still
  has no hard filesystem mutex, which is accepted and honest (ADR-0015).
- **Invariants preserved.** Zero-token deterministic hot path, decay, and
  derived-weight invariants untouched; no network listener; platform-agnostic; and
  now explicitly **mode-agnostic**.

This ADR is **design-only**; the commit-scope guard, the isolated validate/push
helper, and the optional path-hint are a separate downstream implementation task.
