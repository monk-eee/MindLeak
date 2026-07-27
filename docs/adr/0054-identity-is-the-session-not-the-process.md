# ADR-0054: Identity is the session, not the process that hosts it

- Status: Accepted
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Amends: [ADR-0030](0030-discrete-per-agent-identity.md) (discrete per-agent
  identity — the id shape defined there loses its base segment)
- Related: [ADR-0035](0035-fleet-management-heuristics.md) (declared, never
  detected), [ADR-0046](0046-agents-talk-through-the-durable-thread.md) (agents
  talk through the durable thread),
  [ADR-0045](0045-a-fleet-is-a-distributed-system.md) (a fleet is a distributed
  system)

## Context

ADR-0030 made identity a function of the session token, which was right, and
then wrote it as `session:v1:{base}:{fingerprint}`, which was not. The
fingerprint is derived from the token the client mints. The `base` is read from
`LODESTAR_AGENT` / `MINDLEAK_AGENT` **in the environment of the server process**
at startup, and is described in that ADR as "a human-readable base label only".

A label that is only for humans became half of the key. Every comparison in the
system is whole-string equality — `tasks.owner`, `task_qa.audience`,
`pending_questions`, `check_overlap`, the wait graph — so one session hosted by
two differently configured processes resolves to two identities that the system
cannot tell are the same agent.

This is not hypothetical. `fleet_view` on 2026-07-27 reported:

| Agent | Claims |
|---|--:|
| `session:v1:agent:bff9bbe3968f16636cbc5522086114e3` | 1 |
| `session:v1:copilot:bff9bbe3968f16636cbc5522086114e3` | 2 |

One session token. One fingerprint. Two rows, three claims split across them.
Two further agents in the same fleet were split the same way: six rows for three
agents.

The damage runs in three directions:

1. **The board misreports the fleet.** An agent's claims are divided between its
   halves, so no surface can answer what one agent holds. `check_overlap` cannot
   see a collision between two halves of the same agent, and reports a collision
   between two halves that are not really two.
2. **Addressed questions are undeliverable.** `pending_questions` matches
   `audience = ?1` exactly. Address a peer as `session:v1:copilot:…` when its
   server booted without that variable and it answers to `session:v1:agent:…`;
   the question is invisible to its recipient, forever. The task parks, nobody
   owes an answer, and the parking grace expires a week later. This makes
   ADR-0046 — the whole agent-to-agent dialogue capability — undeliverable in
   practice, which is how the defect was found.
3. **Deadlock detection can be evaded by accident.** A wait cycle needs two
   distinct nodes; identities that fork arbitrarily can hide a real cycle or
   invent a false one.

The failure was already visible in the workarounds it forced.
[`DEVELOPERS.md`](../../DEVELOPERS.md) instructed operators that "publishing must
use the same base that claimed", and `scripts/claim-gate.mjs` had grown a
`sessionFingerprint()` helper that compares the fingerprint and ignores the rest.
Two places had independently concluded that the label is not part of the
identity. The code had not.

ADR-0035 already decided this class of question and decided it the other way
round: a session **declares** where it is working, and the server never detects
it, because a stdio server may not share the agent's working directory. Reading
the agent's name out of the server's own environment is that same category
error, and it produced exactly the failure ADR-0035 exists to prevent.

## Decision

**The agent id is `session:v1:{fingerprint}`. A label is never part of it.**

1. **Identity is derived from the session token and nothing else.** The id is
   stable across processes, hosts, restarts, and environments, because the only
   input is the token the client already mints per session.
2. **The label survives as a display name.** `SessionIdentity` carries a `name`
   for reports; `LODESTAR_AGENT` / `MINDLEAK_AGENT` still set it. It is written
   to no key and compared in no query. Two processes disagreeing about it is
   harmless, which is the point.
3. **A migration collapses stored ids rather than stranding them.**
   `session:v1:{name}:{fingerprint}` is rewritten to `session:v1:{fingerprint}`
   across every column that holds an identity. Because both halves of a split
   agent share the fingerprint, the rewrite *heals* the existing damage: the
   three claims above merge onto one agent. `session_context` is keyed on the
   identity, so its duplicate halves are merged to the most recent declaration
   before the rewrite.
4. **Claim recovery takes the name explicitly.** `compatible_legacy_owner` used
   to parse the base back out of the id to decide whether a session could
   recover a claim left by a pre-session owner (`copilot`, `copilot-1a2b3c4d`).
   With no label in the id there is nothing to parse, so the recovering
   session's declared name travels with its id as one value
   (`RecoveringSession`). The guard is unchanged; only its source is honest.
5. **No back-compatible reader.** Nothing accepts both shapes. The migration
   runs on open, so there is no window in which two forms coexist, and a
   dual-form reader would be exactly the permanent "transitional" bridge this
   project refuses.

## Consequences

- **One agent is one row.** `fleet_view`, `check_overlap`, `board` and the wait
  graph agree about who exists. The split agent above becomes one agent holding
  three claims.
- **Addressed questions reach their recipient**, which is the precondition for
  ADR-0046 being usable at all rather than merely implemented.
- **`DEVELOPERS.md`'s "publish with the same base that claimed" instruction is
  deleted, not amended.** It was advice for working around this bug; keeping it
  would preserve the belief that the label matters.
- **Stored ids change shape.** Anything outside this repository that parsed the
  three-part form breaks. That is the honest cost of removing a segment, and it
  is preferred over a reader that accepts both — which would leave the system
  with two spellings of one identity forever, reintroducing the defect in a
  slower form.
- **The id is less readable.** `session:v1:bff9bbe3…` no longer says "copilot"
  at a glance. Reports print the declared name beside it; a key is not a place
  to store something for humans to read, which is the whole lesson here.
- `scripts/claim-gate.mjs`'s fingerprint comparison becomes redundant rather
  than wrong — whole-string equality now means what it says. It is left in
  place, since it still correctly handles rows written before the migration ran.
