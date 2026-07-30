# ADR-0070: Paused work must find its owner or a successor

- Status: Accepted
- Date: 2026-07-29
- Related: [ADR-0020](0020-task-lifecycle-states.md) (paused and
  `needs_input`), [ADR-0046](0046-agents-talk-through-the-durable-thread.md)
  (delivery on calls agents already make),
  [ADR-0052](0052-a-lease-is-a-heartbeat-not-a-deadline.md) (free heartbeat),
  [ADR-0064](0064-the-log-is-the-ledger.md) (transition history),
  [ADR-0067](0067-a-claim-is-a-statement-that-you-are-working-on-something.md)
  (claim cap)

## Context

Four of twenty-seven nonterminal tasks were paused when this decision was made.
Every pull request behind them had merged with all required checks green. Three
had been parked because a stale Lodestar process made conformance untrustworthy;
one only waited for CI. Both owners were still active sessions, but neither had
returned to the work.

The state machine was preserving exactly what ADR-0020 asked it to preserve:
`paused` kept its owner and evidence window, cleared the lease, stayed visible on
the board and became claimable by the pool after seven days. The problem was not
retention. It was delivery.

`pending_questions` and `waiting_on_you` already prove the adopted pattern. A
question that lives only in a thread depends on the addressee remembering to
poll; delivering it on pickup and heartbeat turns it into something the agent
actually sees. Paused work had no equivalent. The owner knew at the instant it
called `pause_task`, and after a restart or task switch the only reminders were a
board it had to remember to inspect and a stalled-work report it had to remember
to run. That is the exact adoption failure ADR-0046 and ADR-0052 measured and
removed elsewhere.

A dead owner had the opposite problem. Ownership is correctly protected during
the seven-day parking grace, so another agent cannot take deliberately suspended
work. But no person could make an explicit, audited exception. The only recovery
was to wait the whole grace even when the owner process was known gone and a
replacement stood ready.

These are one problem, not two: paused ownership has to reach either the owner
who can resume it or an accountable successor. Solving only delivery leaves dead
owners stranded; solving only transfer encourages avoidable takeovers of work an
owner merely forgot.

## Decision

1. **Paused work rides on calls the owner already makes.** `open_session`,
   `claim_task` and `renew_lease` return `paused_by_you` when the resolved session
   owns paused tasks. Each entry names the task, title, `parked_at`, and exact
   pause reason. A bounded instruction says to call `resume_task`.

2. **Absence is omitted, not represented by an empty list.** This matches
   `waiting_on_you`: old clients remain compatible, and a reader does not have to
   distinguish "nothing paused" from "this server does not report pauses".

3. **`needs_input` stays distinct.** A question is resumed by `answer`; a paused
   task is resumed by its owner with `resume_task`. The delivery text names that
   distinction rather than teaching the wrong transition.

4. **Early dead-owner recovery extends the existing `recover_claim` path.** It
   does not add another lifecycle verb. Before `PARKING_GRACE_SECS`, recovery of
   a paused task requires the exact expected owner, a new registered session, a
   non-empty reason and a distinct human reviewer. The transfer opens a fresh
   evidence window for the successor and records the old owner/status/window,
   reviewer and reason in the task event log; the reason is also appended to the
   task thread where a person reading the pause will see it.

5. **The reviewer label is attributable, not authenticated.** Lodestar is a local
   stdio service with no human identity provider. Requiring and recording a
   reviewer makes the decision auditable; it does not prove the named person was
   present. The tool description says so. Pretending a string comparison is
   authentication would be worse than stating the actual boundary.

6. **Agent-only takeover remains impossible before grace.** Without a reviewer,
   paused ownership is unchanged until the existing seven-day grace. Neither the
   old nor new owner may review its own transfer, `needs_input` is not widened,
   stale `expected_owner` is refused, and recovery obeys ADR-0067's concurrent
   claim cap.

7. **The ordinary fallback is unchanged.** After the grace, `next_task` and
   `claim_task` may return/reclaim the parked task as before. No timer rewrites
   rows, no grace is shortened, and no PR/CI event resumes work automatically.

## Consequences

An owner reconnecting after a restart is told immediately what it paused and why.
The reminder repeats on pickup and heartbeat until the task is resumed, so it
cannot disappear merely because the first notice scrolled past.

A person can reassign a genuinely dead owner's paused task immediately and leave
a durable account of doing so. The replacement starts a new evidence window;
the old owner's evidence is history, not silently inherited proof.

The exceptional path can be socially abused because the reviewer label is not
authenticated. That is an existing limitation of local human-review fields such
as `resolve_task`, now made explicit rather than hidden. A future authenticated
human identity mechanism can strengthen the check without changing the state
transition or its audit record.

The seven-day grace remains deliberately conservative for ordinary agents. A
slow human answer, an overnight pause or an agent expected to return should not
become claim theft merely because a shorter timer would make the board look
cleaner.
