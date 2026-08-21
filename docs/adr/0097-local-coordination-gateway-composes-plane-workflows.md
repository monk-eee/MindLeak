# ADR-0097: A local coordination gateway composes plane workflows

- Status: Accepted
- Date: 2026-08-17
- Deciders: MindLeak maintainers
- Accepted: 2026-08-21 by the repository owner, authorized directly in
  session — attributed human adoption after review.
- Amends: [ADR-0066](0066-retrieval-rides-on-the-question-already-asked.md)
  (a second response cue did not create the read-before-write habit)
- Related: [ADR-0044](0044-declared-context-is-durable.md) (Git context is
  declared by the client), [ADR-0054](0054-identity-is-the-session-not-the-process.md)
  (one token identifies one session),
  [ADR-0081](0081-agent-memory-is-a-staging-area-not-a-silo.md) (client memory
  is promoted deliberately), [ADR-0089](0089-mindleak-is-an-operating-system-for-agent-coordination.md)
  (the planes form one coordination system)

## Context

The two local planes have useful mechanisms and no single operational entry
point. A client must call `open_session` once on MindLeak and once on Lodestar,
then remember which preflight, governance, knowledge, and health reads belong
to which server. Sharing a token gives both calls one identity; it does not make
them one call.

That distinction was measured in this repository. On 2026-08-17 the live
Memory Plane reported 45,289 `list_agents` calls, 23,771 `graph_stats` calls,
and 12,664 `telemetry_snapshot` calls. Deliberate pre-edit reads remained tiny,
and every writing session in the bounded memory-habit sample had zero memory
reads. The Intent Plane had 32 active goals and 451 active lessons, but those
facts had to be cross-referenced by hand against Memory Plane telemetry to
answer whether either system changed an agent's behaviour.

ADR-0066 already anticipated this result. After moving retrieval into
`check_overlap` and then adding a `memory_preflight` cue to successful claims,
it said that another failed cohort must lead to first-party client orchestration,
not another reminder. The cohort failed: the telemetry recorded the omission
and nothing in the product acted on it.

Some requested fixes do not belong inside either plane:

- A server cannot verify Git context while ADR-0044 forbids it from inspecting
  Git and a stdio process may not share the caller's working directory.
- MindLeak cannot inline Lodestar knowledge without opening the Intent Plane's
  database, and Lodestar cannot claim MindLeak answered a preflight it never
  called.
- Lodestar cannot watch VS Code's private memory storage without reversing
  ADR-0081's portability and privacy boundary.
- A goal-less task would make durable intent optional inside the model whose
  task contract says work serves intent. Collision avoidance before governance
  is real, but it is a reservation problem rather than a policy claim.

Polling reductions, zero-goal guidance, degraded model health, and node-bound
knowledge on `advise` can improve the current tools independently. They do not
settle who composes them.

## Proposed decision

1. **A first-party local coordinator is the one agent-facing entry point.** It
   is a thin client/gateway over the existing stdio servers, not a third source
   of graph or intent truth. MindLeak remains the Memory Plane and Lodestar
   remains the Intent Plane; neither server opens the other's database.

2. **One coordinator `open_session` call opens both planes.** It forwards the
   same client-minted token and declared context, verifies that both replies
   resolve the same agent and repository id, and returns both build versions,
   plane health, attention queues, and zero-goal startup guidance. A partial
   open names the failed plane and never presents one-plane state as complete.

3. **One preflight composes the questions already asked.** Given paths and
   symbols, the coordinator runs MindLeak `check_overlap`, Lodestar
   `task_query(view="overlap")`, and Lodestar `advise` concurrently. The result
   carries structural impact, live claims, governing clauses, and bounded
   node-matched lessons. A write/claim attempted without a current preflight
   receives a structured review warning and an explicit continue action; the
   coordinator does not pretend it can block filesystem writes made elsewhere.

4. **Git observation stays on the client side and is labelled observed.** The
   coordinator may inspect the workspace's branch, head, dirty state, worktree
   ownership marker, and distance from the declared base, then submit those
   facts through the existing declaration contract. `fleet_view` shows both the
   latest declaration and when the coordinator last verified it. A mismatch is
   a soft collision signal capped at review, never a hard lock.

5. **Fresh repositories get leased scope reservations, not fictional goals.**
   Before a constitution or task exists, `start_work(paths, symbols)` can create
   a short-lived, owner-guarded reservation visible to overlap checks. A
   reservation carries no acceptance criteria, conformance status, completion,
   or implication that policy was adopted. Once a real task is available the
   coordinator replaces the reservation with that task's claim atomically.

6. **Governance bootstrap remains explicit.** The coordinator can offer
   `constitution_define(action="goal")` and structured
   `constitution_define(action="import")`, and can inventory accepted ADRs for
   review. It never activates inferred Markdown as policy and never imports a
   proposal as accepted governance without an attributed decision.

7. **Memory reconciliation uses the portable source contract.** A client adapter
   may submit an atomic repository lesson plus its `/memories/repo/...`
   `source_ref`. Before writing, the coordinator reads the current source and
   reports reconfirm, supersede, or conflict. The server does not watch private
   editor storage, mirror whole files, or copy global user memory into a
   repository database.

8. **One retrospective joins declared value to observed use.** The coordinator
   combines bounded Memory Plane telemetry with Lodestar task, goal, knowledge,
   and conformance counts. It reports call-volume outliers, current failures and
   degraded optional services, read-before-write adoption, claim outcomes, and
   knowledge reads versus writes. Human-facing health notifications are emitted
   on state transitions, not on every poll.

## Consequences

- A solo operator with concurrent agents can reserve paths immediately, while
  the constitution remains honest about whether durable intent was adopted.
- Agents remember one session and one preflight workflow. The coordinator owns
  sequencing; the planes keep their existing storage and authority boundaries.
- Git-backed presence becomes more trustworthy than self-attestation without
  moving Git inspection into a server that may have the wrong working directory.
- Goal-less reservations add a small durable coordination record and expiry
  path. They must not grow task fields one by one; if they need acceptance or
  completion, they are tasks and need a goal.
- Source reconciliation prevents exact client-memory mirrors when the client
  participates. It cannot deduplicate private notes a client never submits,
  which is an honest limit rather than a hidden filesystem dependency.
- The coordinator becomes a reliability boundary. Its result must carry
  per-plane provenance and partial-failure state so composition cannot turn two
  qualified answers into one confident wrong answer.

## Alternatives considered

**Make MindLeak call Lodestar (or the reverse).** Rejected. It creates a server
dependency between planes, couples their availability, and gives one plane an
unreviewed path into the other's database semantics.

**Merge both MCP servers.** Rejected as the first move. A shared executable can
still preserve modules, but it is a packaging migration with a much larger
blast radius than a coordinator and does not itself define Git observation,
source reconciliation, or pre-governance reservations.

**Auto-create an active default goal.** Rejected. It would make policy adoption
an installation side effect and erase the distinction between coordination and
governance that the zero-goal state is correctly expressing.

**Let Lodestar inspect Git directly.** Rejected under ADR-0044. The client has
the relevant checkout and can declare observed facts; the server may be shared
across worktrees and cannot know which one the caller means.

**Watch and mirror editor memory files.** Rejected under ADR-0081. The path is a
private client implementation detail, file boundaries are not lesson
boundaries, and global or temporary notes do not belong in repository intent.
