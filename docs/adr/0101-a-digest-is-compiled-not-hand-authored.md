# ADR-0101: A digest is a compiled view of the graph, not a hand-authored document

- Status: Proposed
- Date: 2026-08-18
- Deciders: Pending human acceptance
- Related: [ADR-0056](0056-the-changelog-is-assembled-not-edited.md) (the
  changelog is assembled, not edited — the direct precedent this generalises),
  [ADR-0049](0049-publication-requires-a-claim.md),
  [ADR-0065](0065-completion-belongs-at-the-publication-boundary.md),
  [ADR-0078](0078-an-unbound-file-is-reported-at-publication.md) (the existing
  "publication" vocabulary this must not collide with),
  [ADR-0090](0090-certification-is-a-status-not-a-service.md) (precedent for a
  current/stale status read), [ADR-0022](0022-learned-knowledge-loop.md) (the
  data source), [ADR-0031](0031-exportable-conformance-evidence.md) (a
  narrower existing precedent for rendering structured state portably)

## Context

An informal Phase 1 audit against this repository's own roadmap (2026-08-18)
found no capability that turns graph knowledge, evidence, and decisions into a
playbook, runbook, repository guide, or weekly report. What exists —
`export_evidence`, `export_conformance_manifest`, `export_constitution` — each
renders one fixed structured record to one fixed format. None of them compile
an arbitrary document from current knowledge: today that is either not done at
all, or done by a human or agent hand-writing prose that drifts the moment the
underlying graph state changes.

This repository already has a precedent for exactly this failure mode and its
fix. ADR-0056 exists because a shared, hand-edited `CHANGELOG.md` went stale
under concurrent agents and caused merge pain; the fix was to assemble it from
`changelog.d/` fragments at release time rather than edit it directly. A
generated document sourced from the knowledge graph is the same shape of
problem at a larger scale: any hand-maintained playbook, runbook, or report
will drift the moment a fact it depended on changes.

The term "publication" is already heavily loaded in this repository. ADR-0049
("Publication requires a claim"), ADR-0065, and ADR-0078 all use it to mean
the git-push/ledger boundary `canonical-push` enforces. Reusing the word for
"render a document from graph state" would make every future reference to
either meaning ambiguous. This ADR deliberately uses **digest** instead.

The trigger is concrete: CompLeak, an external product built on top of
MindLeak, wants to publish compliance playbooks and weekly reports sourced
from MindLeak's knowledge graph. Per this project's own boundary rule — would
this be useful to any MCP agent regardless of domain? — generic digest
generation belongs here; only compliance-specific templates belong in the
consuming product.

## Decision

**A digest is a named, typed, regenerable compilation of current graph state,
rendered through a template — never hand-authored prose that a human or agent
edits directly.**

1. **A digest is a new node type, not a special case of Knowledge or
   Evidence.** It carries a `digest_type` (an open string: `playbook`,
   `runbook`, `repository_guide`, `weekly_report`, `learning_summary`, ...), a
   `generated_at` timestamp, a `source_snapshot` (the node/edge ids and
   weights it was compiled from), and a `template_id`. Its content is
   regenerated output, never authored input.
2. **Digests are compiled, never edited.** Mirrors ADR-0056's "assembled, not
   edited" precedent exactly: no MCP tool exists to hand-edit digest content.
   To change a digest, change what it is compiled from — add knowledge,
   resolve a gap, accept a decision — and recompile.
3. **A template is a deterministic rendering function over typed source data,
   not a prompt.** Each `digest_type` maps to a Rust function taking matching
   knowledge, evidence bundles, decisions, and conformance history, and
   producing Markdown. An optional LLM narration pass may smooth prose
   afterward through the existing consolidation client, but the section list,
   structure, and underlying facts come from the template deterministically —
   consistent with the zero-token-deterministic-hot-path invariant, since
   compilation sits off any hot path and any model call stays optional and
   asynchronous, exactly like `consolidate.rs`.
4. **Regeneration is explicit, not automatic on every write.** A digest
   records the `source_snapshot` it was compiled from. A `digest_status` read
   reports `current` or `stale` by comparing that snapshot against live graph
   state — the same pattern `certification_status` (ADR-0090) already uses to
   report a certified subject as `stale` once it moves past its evidence. It
   never silently regenerates the document; a document changing under a
   reader without notice is worse than a visibly stale one.
5. **Compiling a digest is not publication.** It never touches git and is not
   a `workflow:git.publish` action; it produces graph content only. Turning a
   digest into a committed file remains a deliberate, separate step — writing
   it under `docs/` (or elsewhere) and committing normally — which is exactly
   where ADR-0049's actual publication vocabulary and its claim requirement
   apply, keeping the two concepts related but never conflated.
6. **MindLeak owns generic digest types and the compiler; consuming products
   own domain-specific templates.** MindLeak ships `digest_type`s useful to
   any MCP agent (repository guide, learning summary, weekly retrospective,
   ADR digest). A domain-specific consumer supplies its own `template_id` and
   filter against the same `compile_digest` tool; it never gets its own copy
   of the compiler.

## Consequences

- Two new MCP tools (compile, status) rather than one, in exchange for never
  accumulating a hand-authored document that silently drifts from the graph
  it describes — the same payoff ADR-0056 already measured for the changelog.
- "Digest" must be used consistently from here on; anyone tempted to write
  "publish a digest" must say "compile" or "write it to a file and publish
  that file", never conflate the two meanings this ADR deliberately keeps
  apart.
- The Learning Dashboard and Retrospective Report gaps the same audit found
  become ordinary digest types once this lands, rather than separate bespoke
  tools each reinventing rendering.
- Digest compilation is a new read surface subject to the existing
  "advertised MCP tool surface has a reviewed budget" clause, reviewed at
  implementation time like any other addition.

## Rejected alternatives

- Calling it **Publication**, as the informal design sketch that inspired this
  ADR did — rejected outright: ADR-0049/0065/0078 already define what that
  word means here, and reusing it would make every future ADR ambiguous about
  which meaning applies.
- Storing rendered Markdown as an ordinary `artifact` node — rejected: an
  artifact represents a real repository file MindLeak observed; a digest is
  generated output that may not exist as a file at all, and conflating the two
  would make `ingest_file` re-observe MindLeak's own output.
- Free-form LLM generation with no template — rejected: produces different
  output for identical graph state on every call, failing the reproducibility
  bar `export_constitution`/`export_evidence` already hold themselves to, and
  drifts from the zero-token-deterministic-hot-path spirit for what should be
  a cheap, repeatable read.
