# ADR-0071: Task resolution records an unverified reviewer label

- Status: Accepted
- Date: 2026-07-29
- Decider: MindLeak maintainer (option 2 selected explicitly)
- Related: [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed
  verdicts), [ADR-0047](0047-a-status-is-not-a-decision.md) (a status is not a
  decision), [ADR-0069](0069-resolutions-that-predate-attribution-are-accepted-as-historical.md)
  (historical resolutions),
  [ADR-0070](0070-paused-work-must-find-its-owner-or-a-successor.md) (reviewer
  labels on paused-task transfer)

## Context

`resolve_task` is the one task transition that may overrule a conformance
verdict. It accepts an `in_review` task to `done`, records `resolved_by`, and
pins the conformance record the reviewer accepted.

The argument was named `human`, documented as a "human reviewer identity", and
guarded by two checks:

1. it is not blank; and
2. it is not equal to the agent id in the evidence under review.

Those checks do not establish a human identity. Any caller can satisfy them by
supplying any different string. Lodestar is a local stdio service with no login,
certificate, operating-system identity binding, trusted user directory, or
human identity provider. The plane cannot distinguish `lyndon` entered by
Lyndon from the same label entered by an agent.

The old wording therefore implied authentication where only attribution
existed. That matters because a completed task reads as a human judgement
superseding evidence. An unverified label may still be valuable audit data, but
calling it an identity makes the record appear stronger than the mechanism that
created it.

The maintainer chose option 2 explicitly: retain the local label model and make
its trust boundary honest. Option 1 -- verifiable human identity -- would require
an identity source and a policy for unavailable/failed verification; inventing
one inside an agent task is not permitted.

## Decision

1. **`resolve_task` accepts a reviewer label, not a human identity.** The value
   remains a non-empty string and is persisted unchanged in `resolved_by`.

2. **The label is attributable, not authenticated.** Core documentation, error
   messages, MCP tool descriptions, and evidence/board exports must use wording
   that does not imply Lodestar verified who supplied it.

3. **Same-string self-review remains forbidden.** When the latest conformance
   evidence identifies agent `A`, reviewer label `A` is refused. This is a
   useful guard against the acting agent naming itself; it is not proof that any
   other label names a person.

4. **Arbitrary distinct labels are valid and test-pinned.** A label need not
   resolve to a known principal. The regression records a deliberately
   non-credential label in `resolved_by`, proving the contract is attribution
   rather than authentication.

5. **Historical nulls keep their ADR-0069 meaning.** `resolved_by IS NULL` on
   completed tasks still means the resolution predates attribution. No label is
   invented retroactively.

6. **This decision does not weaken evidence.** `resolve_task` remains available
   only for `in_review`, after a persisted conformance check. The reviewer label
   describes who was declared to have accepted that result; it does not change
   the verdict, evidence, or pinned conformance record.

## Consequences

Audit readers can interpret the record correctly:

- `resolved_by = "lyndon"` means the caller declared `lyndon` as reviewer and
  Lodestar recorded that label;
- it does not mean Lodestar authenticated Lyndon;
- equality with the reviewed agent id is refused, but a distinct label is not
  verified.

This is intentionally weaker than authenticated approval and stronger than an
anonymous transition. It matches the actual authority of a local, unauthenticated
stdio process.

A future authenticated reviewer mechanism can supersede this decision by adding
a verifiable principal and provenance for that verification. It must not silently
reinterpret existing labels as authenticated identities; historical records
remain unverified declarations.

The same wording applies to other local human-review labels, including the
reviewer on ADR-0070 paused-task transfer. This ADR decides `resolve_task`; it
does not create a general identity provider.
