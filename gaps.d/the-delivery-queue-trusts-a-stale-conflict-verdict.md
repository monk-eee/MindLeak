- **`delivery-queue.mjs` treats GitHub's cached `DIRTY` as an authoritative
  conflict verdict, and so refuses a branch that merges cleanly — OBSERVED
  2026-08-13 on PR #419, left OPEN.** The queue reads `mergeStateStatus` from
  `gh pr list`. That field is a cached answer GitHub recomputes lazily, and
  after a burst of merges moves the base it can report `DIRTY` for a branch
  that has no conflict at all. The queue believes it and stops.

  Three ways of asking the same question about `docs/ackplane-has-its-own-goal`,
  within one minute of each other:

  | Question | Answer |
  |---|---|
  | `gh pr list --json mergeStateStatus` — what the queue reads | `DIRTY` |
  | `git merge-tree --write-tree --name-only origin/main origin/<branch>` | exit 0, clean tree `198b4783` |
  | one `gh api repos/monk-eee/MindLeak/pulls/419` | `mergeable: true`, `mergeable_state: behind` |

  Only the first was wrong, and it is the only one the queue consults.

  The consequence is worse than an ordinary skip, because a conflict is the one
  case the queue is designed to hand back: it printed `#419 has a real conflict;
  it needs its own worktree, not the queue` and moved on. That message is
  correct behaviour applied to a false premise, so it reads as the system
  working. The owner is directed to go and resolve a conflict that does not
  exist, in a worktree, which under
  `goal:an-agent-commits-only-in-a-working-tree-it-owns` only they may enter —
  so nobody else can clear it for them either.

  Nothing in the loop ever rechecks. The queue re-reads the same cached field on
  every pass and refuses again, so the branch is stranded not for a bounded
  interval but until a person notices and asks a different question. Here the
  queue refused it on two separate runs; a single REST `GET` corrected the
  verdict, after which the queue updated the branch unaided and it merged as
  `8c89f723`, seventeen minutes later, with no conflict resolution performed by
  anyone. The stale verdict also survived the branch's own merge of `main`,
  which is what makes it hard to spot: the owner can do everything right and
  still be told they have a conflict.

  The fix direction is cheap and deterministic. `git merge-tree --write-tree`
  answers the question locally without touching a worktree, index or ref, so the
  queue can verify a `DIRTY` before acting on it, and treat only a genuine
  conflict as a hand-back. Forcing GitHub to recompute is the weaker option: it
  needs the REST pulls endpoint rather than the GraphQL list, and `mergeable`
  arrives `null` while it computes, so it has to be polled.

  Left for later rather than fixed here, because this run was already carrying
  the pull requests the defect had stalled and changing the queue's refusal
  logic while draining it would have validated the change against the very
  branches it was meant to arbitrate. Recorded durably as
  `knowledge:bd5d1dd19a5e` against `artifact:scripts/delivery-queue.mjs`, so it
  reaches whoever edits the queue next.
