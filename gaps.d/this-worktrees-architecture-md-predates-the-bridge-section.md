- **What**: this worktree's `docs/ARCHITECTURE.md` has no "Bridge" section
  at all -- no mention of `ackplane-bridge`, its route table, or any of the
  domains it serves (Fleet, Work, Knowledge, Delegation, Context, Evidence,
  Telemetry, Live Feed, Design). `origin/main`'s copy of the same file has
  an extensive, current Bridge section (route table, auth model,
  read-vs-mutation split) that this branch's checkout predates.
- **Where**: `docs/ARCHITECTURE.md` in worktree
  `MindLeak-industrial-design-materializations` (branch
  `feat/industrial-design-materializations`), compared against
  `origin/main` at `73c27795`.
- **Impact**: the new Design mutation routes (ADR-0123) could not be added
  to the Bridge route table from this worktree without either duplicating
  or contradicting whatever `main` already has -- the file needs a rebase
  onto current `main` before any Bridge-section edit is safe here, not a
  standalone append.
- **Not fixed this run**: rebasing a long-lived worktree with substantial
  uncommitted local changes onto a fast-moving `main` mid-session is a
  separate, real operation with its own conflict risk -- left for a
  dedicated rebase pass before this branch's PR is opened, or as a
  follow-up once the Design mutation slice itself has landed.
