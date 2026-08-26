# ADR-0052: A lease is a heartbeat, not a deadline

- Status: Accepted
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Related: [ADR-0030](0030-discrete-per-agent-identity.md) (per-agent identity),
  [ADR-0048](0048-a-lapsed-lease-holes-the-window-it-does-not-move-it.md) (a
  lapsed lease holes the evidence window),
  [ADR-0049](0049-publication-requires-a-claim.md) (publication requires a claim)

## Context

`claim_task` defaults to a **300-second** lease. Nothing renews it. An agent that
claims work, edits several files, runs a test suite, and commits has lost its
claim long before it reaches `complete_task`.

This is not theoretical. In one session on 2026-07-27 the board carried six
claimed tasks; `stalled_work` reported four as stalled, three of them
`lapsed_lease`:

| Task | Lease lapsed | Actual state of the work |
|---|---|---|
| `e20b25c9d588` | 54 min before it was noticed | shipped, PR merged |
| `26965fe7c7a0` | 30 min | shipped |
| `733b882db4ab` | **200 seconds after being claimed** | shipped |

`733b882db4ab` is the clearest case: the lease expired barely over three minutes
into the work. Every one of those tasks was *finished* — the code was written,
reviewed and merged. What failed was the claim outliving the activity it
described. The board read "six agents are working"; the truth was "three agents
finished and one thing is waiting on a human".

The consequences compound rather than stay cosmetic:

1. **The receipt is lost.** A lapsed lease cannot be renewed, and re-claiming
   opens a fresh evidence window (ADR-0048), so commits made during the original
   window fall outside it. The honest verdict becomes `needs_human` with *"no
   provenance-bearing mutation"* — the contract correctly refusing to certify
   work it can no longer bound. Real work, no receipt.
2. **The board misreports the fleet.** Claimed-but-lapsed is indistinguishable
   at a glance from claimed-and-active, so `check_overlap` warns about
   collisions with agents that stopped working an hour ago.
3. **It trains people to widen windows.** The tempting repair is to back-date
   `started_at` until the commits fall inside — precisely the rationalisation
   the evidence contract exists to prevent.

Raising the default would only move the cliff. A ten-minute lease still expires
during a long test run, and a one-hour lease keeps a crashed agent's claim alive
for an hour — which is the reason leases are short in the first place.

## Decision

**A lease is renewed by evidence of activity, not by a timer the caller must
remember.**

1. **Any authenticated call that names a task renews its lease**, as a side
   effect. `task_scope`, `ask_question`, `answer`, `conformance_history`,
   `advise` and `check_conformance` are all proof the owner is still working.
   The heartbeat becomes free: an agent doing its job cannot lose its claim, and
   an agent doing nothing still loses it on schedule.
2. **`renew_lease` remains**, for the genuine case of a long silent operation —
   a build, a model call — where the agent is busy but not calling anything.
3. **The default lease stays short.** The point of a short lease is that a
   vanished agent frees its work quickly. Renewal-on-activity keeps that
   property while removing the failure mode, where raising the default would
   trade one for the other.
4. **A renewal never extends the evidence window backwards.** It moves
   `lease_expires_at` only. `claim_started_at` is untouched, so the window
   still bounds exactly what the claim covered (ADR-0048 is unaffected).
5. **Renewal is owner-only and silent on failure.** A call from a non-owner, or
   against an already-lapsed lease, does not renew and does not error — the call
   does its own job. A lapsed lease still requires a deliberate re-claim, so
   this cannot resurrect a claim someone else has taken.

## Consequences

- An agent that works continuously keeps its claim, and the receipt it
  eventually produces covers the work it actually did.
- `stalled_work`'s `lapsed_lease` becomes a real signal — an agent that stopped
  — rather than the routine outcome of doing anything slowly.
- **A wedged-but-chatty agent holds its claim indefinitely.** This is the cost.
  An agent looping on reads renews forever without progressing. `stalled_work`
  cannot see it, because from outside it is indistinguishable from working. The
  mitigation is not a shorter lease but the existing human path: `check_overlap`
  shows who holds what, and a person can `abandon_task`. Naming it here rather
  than pretending renewal-on-activity is free.
- Every task-bearing tool acquires a write, which is a real change for calls that
  are otherwise pure reads. `advise` in particular is documented as evidence-free
  and recording nothing (ADR-0029); renewing a lease from it would contradict
  that. **`advise` should be excluded**, or ADR-0029 amended — this decision does
  not get to quietly redefine another one.

## Not implemented in this build

Nothing here is built. The three tasks above were closed out honestly rather
than by working around the lease, and this ADR records what should change so the
next session does not rediscover it from the same symptoms.

## Amendment (2026-08-26): the short default did not survive contact with real work

- Decider: MindLeak maintainer

The Decision above shipped: `touch_named_task` (`crates/lodestar-mcp/src/tools/mod.rs`)
renews on `task_query view=scope`, `task_transition to=ask`, `task_transition
to=answer`, `conformance_history`, and `check_conformance`, with `advise`
deliberately excluded per the reasoning already recorded in this ADR. Decision
item 3 — keep the default lease short, and let renewal-on-activity carry the
load — did not.

**Measured since:** `HEARTBEAT_ACTS` only covers five specific Lodestar-side
calls. The dominant shape of real work — editing files, running a build or
test suite, `git commit` — never calls Lodestar at all, so nothing renews the
lease during exactly the interval this ADR was written to protect. AGENTS.md
now records 27 measured lapses across 24 tasks and roughly 100 hours of work
sitting under a dead lease. The concrete failure mode this produces is worse
than the one Decision item 3 was protecting against:
`gaps.d/rescuing-a-lapsed-lease-can-duplicate-a-published-pr.md` records a
task whose owner had already pushed a validated fix and opened a PR two
minutes before its 300-second lease lapsed; a second agent, reading the lapse
as abandonment, rescued it and published a byte-identical duplicate PR
fourteen minutes later.

**Revised decision.** The default lease (`DEFAULT_LEASE_SECS`, previously
`300`) is now **1800 seconds (30 minutes)** — long enough to comfortably
outlast one realistic edit/build/test cycle between Lodestar touches, applied
uniformly to `claim`, `renew`, `recover`, `resume`, and `answer`. The
heartbeat mechanism from the original Decision is unchanged and still fires
on top of this; it stops being the *only* thing standing between routine work
and a false lapse.

This does trade away part of what Decision item 3 bought: a genuinely
vanished agent's work now takes up to 30 minutes to free instead of 5. That
cost is accepted because the mitigation for it already exists and does not
depend on lease length — `check_overlap`/`stalled` already surface a
wedged-but-chatty claim to a human (this ADR's own Consequences section named
that case as a cost accepted under the *original* decision too), and a human
can `abandon_task` it regardless of how long the lease runs. What the short
default bought — fast reclaim of dead work — was real but smaller than what
it cost: certified work relabelled as a duplicate-PR incident.

Nothing here loosens what a claim covers. `claim_started_at` is still
untouched by renewal (Decision item 4), and a lapsed lease still requires a
deliberate re-claim (Decision item 5) — only the length of the window before
a live claim can lapse in the first place has changed.
