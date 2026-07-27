# ADR-0038: Isolated worktrees, shared repository state, reviewed convergence

- Status: Accepted
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Supersedes: [ADR-0032](0032-single-checkout-fleet-integration.md)
  (single-checkout fleet integration)
- Refines: [ADR-0035](0035-fleet-management-heuristics.md) (declared per-session
  branch and head context)
- Related: [ADR-0030](0030-discrete-per-agent-identity.md) (session identity),
  [ADR-0024](0024-preflight-overlap-detection.md) (overlap advice),
  [ADR-0025](0025-authoritative-checked-conformance.md) (commit evidence),
  [ADR-0034](0034-typed-controls-and-enforcement-ceilings.md) (mechanical versus
  advisory enforcement)

## Context

ADR-0032 diagnosed a real failure: routine cherry-picking, moving refs beneath
dirty files, and publishing from stale lineages gave one logical change multiple
commit identities. MindLeak evidence is anchored to commit SHA, so history
rewrites split provenance from the commits reviewers actually merge.

It selected the wrong physical remedy. Requiring every agent to share one
checkout also makes every agent share files, branch selection, and a staging
index. In practice that caused accidental commit sweeps, mixed validation,
branch-switch contention, and unrelated publication waves blocking each other.
A linked worktree preserves commit identity; cherry-pick, rebase, and squash do
not. Filesystem topology was made load-bearing when history topology was the
actual invariant.

MindLeak's intended product model has four distinct authorities:

1. **Git worktrees isolate execution.** Each agent edits, stages, validates, and
   commits in its own checkout and branch.
2. **Lodestar coordinates intent.** One shared plane owns task claims, declared
   scope, policy, conformance, and human review.
3. **MindLeak shares learning.** One shared graph carries repository context,
   evidence, and durable learned signal across all agent worktrees.
4. **Protected pull requests converge history.** A branch may publish its exact
   commits, but only a reviewed PR merge may advance `main`.

The current storage defaults do not implement this model. Lodestar resolves
through Git's common directory, MindLeak resolves relative to each workspace,
and extension/installer configuration overrides both with worktree-local paths.
Two agents can therefore have isolated files only by accidentally forking their
coordination and memory databases.

## Decision

### 1. A worktree is the agent's physical isolation boundary

Each concurrent task or workstream uses its own linked worktree and branch.
Agents do not share a writable checkout, staging index, or current branch.
Lodestar claims remain the logical coordination authority: worktree isolation
contains a collision but does not prove scopes are independent.

The same branch may be checked out in only one worktree, as Git already enforces.
One task normally owns one branch; a deliberate multi-task branch is a reviewed
coordination choice, not the default.

### 2. One clone has one stable repository identity

Every non-bare clone receives a 128-bit lowercase hexadecimal repository id.
The authoritative value is stored in shared local Git configuration:

```text
git config --local mindleak.repositoryId <32-lowercase-hex>
```

Linked worktrees share the Git common directory and therefore this config.
Independent clones receive independent ids by default, even when they have the
same remote URL. Remote URLs are mutable, may contain credentials, and do not
express whether two clones should share operational state, so they are never an
identity key.

Initialisation is concurrency-safe. A small marker in the Git common directory
is created atomically to ensure simultaneous first starts choose one id; the id
is then registered in local Git config. The config value is authoritative after
bootstrap, while the marker permits recovery from an interrupted first write.

### 3. Repository state lives in a platform-local user store

Both planes resolve beneath one root, partitioned by repository id:

```text
<local-state-root>/MindLeak/repositories/<repository-id>/
├── graph.db
└── spec.db
```

Defaults are platform-native and non-roaming:

- Windows: `%LOCALAPPDATA%\MindLeak`
- Linux: `$XDG_STATE_HOME/mindleak`, else `~/.local/state/mindleak`
- macOS: `~/Library/Application Support/MindLeak`

`MINDLEAK_HOME` overrides the root for containers, SSH, test harnesses, and
managed environments. Existing `MINDLEAK_DB` and `LODESTAR_DB` overrides remain
highest priority and select a database directly.

Within a Git clone, both servers resolve identity from the Git common directory;
session branch/head/base remain caller-declared per ADR-0035. Detecting stable
repository storage is not permission to infer mutable session state.

Outside Git, each plane retains a workspace-local fallback so scratch use stays
functional without inventing a durable repository identity.

### 4. Legacy local databases migrate without destructive moves

On first use of the repository store, if the destination database is absent and
the corresponding legacy repository-root database exists, the server creates an
online SQLite backup at the new destination. The legacy file is left untouched.
Migration is idempotent and refuses to overwrite an existing destination.

Operators must stop pre-upgrade servers before migration; otherwise an old
process can continue writing the legacy file after the snapshot. The resolved
repository id and database path are emitted at startup so split-state diagnosis
is direct rather than inferred.

### 5. Any clean worktree may publish its own exact branch

`canonical-push.mjs` no longer privileges the primary checkout. It may publish
from a linked worktree only when all of the following hold:

- `HEAD` is attached to a non-protected branch;
- the worktree and its index are clean;
- the destination is the current branch of the same name;
- the remote branch is absent or an ancestor of local `HEAD`; and
- the exact local `HEAD` is pushed, without an alternate commit or destination.

Cherry-pick, rebase, squash, and ref movement are not routine integration tools
because they replace evidence-bearing commit identities. A reviewed merge commit
or fast-forward preserves the original commits.

### 6. Protected main advances only through a pull request

No MindLeak script publishes directly to `main` or `master`. Branch protection
and the hosting provider's PR merge are the mechanical mainline authority. The
supported merge mode preserves source commits; squash/rebase merge is outside the
evidence contract.

Lodestar proves that claimed work was checked and reviewed. MindLeak proves what
executions and commits occurred. Neither plane bypasses repository review, and a
green PR does not fabricate conformance that was never recorded.

### 7. User-local state has an explicit lifecycle

The store concentrates sensitive repository context outside the clone, so it is
user-readable only, never placed in roaming/cloud-synchronised storage by
default, and never keyed by a credential-bearing URL. A repository status command
must expose the id and resolved paths; later cleanup may list and prune orphaned
repository ids by last access. Deleting a clone does not silently delete its
operational history.

## Acceptance gates

1. Two linked worktrees of one clone resolve the same repository id, `graph.db`,
   and `spec.db` without explicit database configuration.
2. An independent clone resolves a different id and state directory.
3. Simultaneous first starts converge on one id.
4. Explicit database and home-root overrides remain deterministic.
5. Existing repository-local databases migrate by verified SQLite backup and are
   not deleted.
6. A linked worktree publishes its current branch's exact `HEAD`; direct main,
   dirty state, detached `HEAD`, and remote divergence are refused.
7. Two isolated agent sessions see the same Lodestar task state and MindLeak
   evidence while retaining distinct branch/head context.
8. A protected PR merge preserves the evidence-bearing source commits.

## Consequences

- Physical clobbering is removed from the multi-agent workflow without weakening
  task claims, overlap advice, or review.
- No checkout is privileged merely because it owns the common `.git` directory.
- Commit identity, conformance evidence, and merged history remain aligned.
- Independent workstreams can publish and review without waiting for an unrelated
  checkout to become available.
- Storage identity, migration, privacy, and cleanup become explicit product
  responsibilities.
- Existing `.mindleak` and `.lodestar` directories remain recoverable migration
  sources but stop being the default live databases after upgrade.

## Rejected alternatives

- **One shared checkout.** It treats a mutable filesystem as coordination and
  recreates the collisions Lodestar exists to manage.
- **Key state by workspace path.** Linked worktrees fork state and moving a
  checkout changes identity.
- **Key state by remote URL.** URLs are mutable, may expose credentials, and
  collapse independent clones without consent.
- **Store live SQLite under `.git`.** It couples application lifecycle to Git
  internals and makes repository maintenance and backup assumptions unsafe.
- **One global database for every repository.** It expands privacy blast radius,
  creates cross-repository key collisions, and makes selective cleanup harder.
- **Permit squash/rebase publication and repair evidence later.** Reconstructed
  provenance is narration about a new commit, not proof attached to the commit
  that was checked.
