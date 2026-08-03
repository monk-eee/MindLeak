# ADR-0081: Agent memory is a staging area, not a silo

- Status: Accepted
- Date: 2026-08-03
- Deciders: MindLeak maintainers
- Related: [ADR-0038](0038-isolated-worktrees-shared-repository-state.md)
  (repository-scoped shared state),
  [ADR-0053](0053-the-graph-records-events-not-conclusions.md) (conclusions are
  supplied, not inferred), [ADR-0080](0080-knowledge-is-searched-where-it-is-already-read.md)
  (knowledge is searched through its existing read surface)

## Context

Agents keep useful notes in client-managed memory. In VS Code those notes are
addressed as `/memories/repo/...`, `/memories/session/...`, and global
`/memories/...` files. They persist in the client's private workspace storage,
not in this repository's Lodestar `spec.db`, and no supported API notifies
Lodestar when one changes.

That separation is valuable for scratch work but wrong for reusable repository
learning. A note can guide the agent that wrote it while remaining absent from
`active_knowledge`, conformance advice, other clients, and every later agent.
Session notes can disappear at the end of the conversation without ever
crossing that boundary.

Blind mirroring is not the answer. A memory file may contain temporary plans,
private preferences, stale measurements, credentials, or several unrelated
lessons. Lodestar knowledge is repository-scoped, attributed, decaying, and
expected to carry provenance that lets it reach future work.

## Decision

Agent memory is a **staging area**. Reusable repository knowledge is promoted
deliberately into Lodestar rather than left in a client-only silo.

1. **Repository memory is promoted at write time.** When an agent writes a
   reusable fact under `/memories/repo/`, it also calls the existing
   `record_knowledge` tool with an atomic statement, a stable `source_ref`, its
   registered session, and evidence naming artifact/symbol nodes or a goal.
2. **Session memory is reviewed before completion.** Reusable repository facts
   under `/memories/session/` are promoted before the session is handed off or
   cleared. Temporary plans and stale observations remain scratch.
3. **Global memory is private by default.** `/memories/*.md` user preferences
   are not copied into a per-repository ledger. A future global knowledge plane
   needs its own scope and decision; duplicating those notes into every
   `spec.db` would leak and fragment them.
4. **The source is logical, not a filesystem dependency.** Lodestar stores the
   portable `/memories/...` reference supplied by the client. It never reads or
   watches VS Code's private `workspaceStorage` layout.
5. **One source has one current lesson.** Exact repeats reconfirm the current
   row. When source text changes, the source moves to the successor and the
   previous lesson is retired once no other source still names it. Retirement
   preserves what was believed and who superseded it.
6. **A sourced write must be deliverable.** It is refused unless its evidence
   reaches artifact/symbol nodes, a goal, or a known task. The public reply says
   which source was stored and which knowledge record it superseded.
7. **No new MCP verb.** Source-aware persistence extends `record_knowledge` and
   source lookup extends `active_knowledge`. The feature belongs on the paths
   agents already use, not behind another command they must remember.

## Consequences

- `knowledge_sources` maps a stable logical source to its current knowledge id.
  It is repository-local and cascades when decayed knowledge is pruned.
- Sourced writes require a registered session for attribution. Unsourced
  `record_knowledge` remains backward compatible.
- Classification stays with the agent that holds the context. This adds no LLM
  call to deterministic ingestion and does not parse arbitrary Markdown into
  policy.
- Updating one source does not retire an identical lesson still named by
  another source.
- Deleting a note is an explicit act through the existing attributed
  `retire_knowledge(source_ref=...)` path. It detaches only that source and
  retires the lesson after its final source disappears.
- The canonical MindLeak skill must report how many reusable memories it
  promoted, and must not claim a flush succeeded when `surfaces` is false.

## Rejected alternatives

**Watch the editor's memory directory.** Rejected because the resolved path is a
private, workspace-hashed implementation detail of one client. It is not a
portable contract for the CLI, other editors, or future VS Code builds.

**Mirror every memory file verbatim.** Rejected because file boundaries are not
knowledge boundaries, and global/private/scratch content does not belong in a
repository ledger.

**Add `sync_memory` as another MCP tool.** Rejected under ADR-0059. The existing
write/read vocabulary already expresses the operation; adding source identity
there keeps one canonical path.
