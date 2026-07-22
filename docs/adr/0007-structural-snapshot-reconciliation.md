# ADR-0007 - Structural snapshots replace owned facts

- **Status:** Accepted
- **Date:** 2026-07-22

## Context

Source ingestion currently only upserts nodes and edges. If a function, call, or
import disappears from a file, re-ingestion never retracts the old fact. Decay
does not make this correct: the graph continues to assert structure that the
latest source snapshot disproves, and later reinforcement can keep that false
fact alive.

Structural extraction and episodic telemetry have different lifecycles:

- a file parse is an authoritative snapshot of that file's current structure;
- an execution, failure, commit, decision, or observation is historical evidence
  that remains true after the event and should fade through decay.

Treating both as append-and-reinforce conflates current truth with history.

## Decision

- Every extractor-owned structural edge records a nullable `owner_id`, normally
  the `artifact:<path>` whose snapshot emitted it.
- `GraphStore::replace_structure` reconciles one owner's complete node/edge
  snapshot in a SQLite transaction: upsert desired nodes and edges, retract
  previously owned edges absent from the snapshot, then remove stale symbol
  nodes only when no surviving edge references them.
- Structural relations (`contains`, `calls`, and ADR-0006's `imports`,
  `depends_on`, `extends`, and `implements`) use snapshot replacement.
- Episodic relations (`modified`, `failed_on`, `refactored`, `relates_to`, and
  `observed`) remain append-and-reinforce and have no structural owner.
- Focus changes node attention and emits `observed` evidence; it does not refresh
  the decay clock or weight of unrelated incident edges.
- Effective weight remains derived at query time. Ownership is provenance, not a
  cached score.

## Migration

Fresh databases include `edges.owner_id` and an owner index. Existing
`contains` edges can be backfilled from their artifact source. Other structural
edges acquire ownership when their source file is next ingested; legacy in-file
`calls` are claimed from their unambiguous symbol prefix before diffing. Schema
upgrades run under an immediate write transaction so concurrent processes cannot
race additive migrations, and interrupted signal backfills are repaired on every
open.

## Consequences

- Removing or renaming source structure is visible immediately after the next
  ingest instead of waiting for decay.
- Reconciliation must be atomic: a failed extraction/write leaves the previous
  snapshot intact.
- A historical edge may keep a removed symbol node as provenance. That node is
  no longer presented as current structure because its owned structural edges
  are gone; once the historical edge decays, pruning removes the orphan symbol
  and its FTS entry.
- Structural ownership cannot transfer silently. A conflicting owner aborts the
  replacement transaction.
- New structural extractors must emit a complete owner snapshot and reuse the
  same reconciliation path; they must not invent relation-specific cleanup.
- Snapshot replacement and temporal decay remain complementary: replacement
  corrects disproven structure, while decay removes old evidence and unattended
  facts.