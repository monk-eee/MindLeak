# ADR-0127: Agent-issued shell commands are auditable evidence, not just prose discipline

- Status: Proposed
- Date: 2026-08-25
- Deciders: Pending human acceptance
- Related: [ADR-0026](0026-constitutional-policy-over-mechanistic-ratchets.md)
  (constitutional policy over mechanistic ratchets),
  [ADR-0034](0034-typed-controls-and-enforcement-ceilings.md) (typed controls
  and enforcement ceilings — the false-safety concern this ADR is careful
  not to repeat),
  [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed
  conformance)

## Context

`AGENTS.md` states a concrete rule: an agent must never pipe or chain
PowerShell-native cmdlets around a git/cargo/npm invocation, because doing so
has repeatedly corrupted encoding or silently mis-reported a command's real
exit code. The rule is precise, it is not new, and it is not disputed.

It is also, today, pure prose. No clause represents it, so no Control could
ever be bound to it, so `node scripts/control-coverage.mjs` cannot even list
it as a gap — it is invisible to the constitution one full layer below
where that tool looks. The only record of a violation is an agent's own
memory notes, written after the fact, by the same agent that just broke the
rule: dozens of independently recorded relapses of this exact pattern exist
across many sessions in this repository's own accumulated memory, each one
proof that self-report is not the same thing as enforcement.

ADR-0026 decision 2 already describes what a mechanism needs to be worth
something here: *"Controls are subordinate evidence mechanisms... reference
an active clause... A control with no active governing clause cannot
hard-block work."* Today's ingestible evidence kinds are commits
(`ingest_commit`) and command executions the agent explicitly chooses to
report (`ingest_execution`). Neither answers "what did this session actually
run," because both depend on the agent deciding to tell the system —
exactly the dependency this ADR exists to remove.

## Decision

1. **A session's tool-call history becomes a new evidence kind,
   `tool_invocation`**, alongside `execution` and `commit`. Each record
   carries the session/agent id, the tool name, a bounded excerpt of its
   arguments (for a terminal-executing tool, the command string itself,
   truncated to a fixed byte ceiling — the same bounded-response discipline
   this codebase already applies everywhere a payload could otherwise grow
   without limit), and a timestamp.

2. **Ingestion is passive, not self-reported.** It sits at the same seam the
   existing passive Git sensor already occupies for git operations — the
   agent never calls a tool to declare "I ran this command"; whatever
   already mediates the tool call records it as a side effect. A rule
   enforced by the agent's own willingness to confess is precisely the
   mechanism this ADR replaces, so the new one must not repeat that shape.

3. **A deterministic, zero-token pattern check classifies each
   `tool_invocation` record** against a small, versioned, committed list of
   banned shapes (a piped PowerShell cmdlet around a native command,
   `$LASTEXITCODE`, and the small number of other patterns AGENTS.md already
   names) — pattern matching only, the same zero-token discipline every
   other ingest path in this crate already holds to. No model call is
   permitted on this path.

4. **A new control, `control:agent-command-hygiene`, reports these
   classifications against a new, distinct clause** —
   `agent-shell-commands-avoid-shell-specific-plumbing`, not the existing
   `committed-instructions-carry-no-shell-specific-p`. That clause governs
   what is *committed*; this one governs what an agent *runs*, live. Binding
   one control to both would make it answer a question neither clause
   actually asks.

5. **The new clause's declared consequence starts at `review`, not
   `block`.** ADR-0034 already named the failure mode of a clause claiming
   more enforcement strength than its control can deliver — a control that
   can only ever report after the command has already run must never be
   attached to a clause that claims to stop it beforehand.

6. **This is retrospective, and is not claimed to be anything else.** The
   architecture is deliberately cooperative, never preemptive, and nothing
   in this ADR proposes changing that. What changes is that a violation
   becomes a durable, queryable, replayable fact — visible in
   `conformance_history`, visible to `control-coverage.mjs`, available to a
   human on request — rather than something only recoverable from an
   agent's own notes, if it wrote any.

7. **Raw command excerpts decay like every other structural fact** (the
   same half-life/prune mechanism already governing every other MindLeak
   node type), rather than being retained forever or discarded immediately.
   They live long enough to be reviewed and then fade, matching the existing
   retention model instead of introducing a second one.

## Consequences

- The first real answer to "did the agent actually follow this" for a rule
  that today exists only as text an agent might reread — measured, not
  assumed.
- A genuine, narrow scope question the implementation must resolve
  carefully, not wave at here: which tools' invocations get recorded, and
  exactly how an argument excerpt is bounded and, where warranted, redacted
  — a raw command string can incidentally carry something (a path, an
  accidentally-inlined value) that should not be retained indefinitely even
  under decay.
- The mechanism generalises to any future "did the agent actually do X"
  question — did it call `generate_test` before writing a test file, did it
  call `check_overlap` before its first edit — without a new evidence kind
  each time. Only the pattern list grows; the ingestion seam does not need
  to be rebuilt per rule.
- No change to what an agent is *able* to do in the moment. This closes the
  audit gap, not the prevention gap; a future decision to add real
  preemptive enforcement (blocking a tool call outright) is a materially
  larger, separate question this ADR deliberately does not answer.

## Rejected alternatives

**Keep relying on self-reported `learned` notes when an agent notices its
own violation.** This is the status quo, and it is precisely the mechanism
this ADR replaces: it only ever catches what the agent happens to notice,
which the accumulated memory record shows is not reliable.

**Block the tool call outright at the harness level.** Out of scope here:
the harness executing tool calls is not this repository's code, and this
ADR is about making the fact auditable within Lodestar/MindLeak, not about
changing a different system's runtime behaviour. Named explicitly so
accepting this ADR is not mistaken for solving that larger problem too.

**Fold this into the existing `committed-instructions-carry-no-shell-
specific-p` clause instead of adding a new one.** Rejected because the two
govern different things at different times — one static text at commit
time, one live behaviour during a session — and a single control trying to
answer both would need to distinguish them internally anyway, which is
exactly what having two clauses already does for free.
