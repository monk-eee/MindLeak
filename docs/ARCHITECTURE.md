# Architecture

MindLeak is **an operating system for agent coordination**
([ADR-0089](adr/0089-mindleak-is-an-operating-system-for-agent-coordination.md)),
implemented as two independent planes with different lifetimes and authority.
The Memory Plane (`mindleak-*`) is a **Temporal Context Graph Engine (TCGE)**
whose episodic edges decay. The Intent Plane (`lodestar-*`, ADR-0004) is a
durable constitution, design, coordination, knowledge, and conformance ledger.

The category is cooperative, not preemptive: the system schedules, arbitrates,
remembers, governs, and audits, but it never preempts an agent, sandboxes a
process, or blocks a write. What ships today is **MindLeak Core**, the local
tier described below. *Ackplane* (federation, ADR-0082 to ADR-0088) and *the
Bridge* (assurance operations, ADR-0090) are accepted designs whose services
are not yet fully built. Ackplane now includes the repository-side contract and
the ledger-backed node synchronization transport described under
`ackplane-core` and `ackplane-server` below.

Each plane is a Rust library behind its own MCP stdio server. They share a
repository identity and session identity, but not tables, database connections,
or hidden in-process state. A client relays typed payloads between them — most
importantly, bounded `evidence_for` output from the Memory Plane into Lodestar
conformance — so the cross-plane contract stays explicit and auditable.

```
MCP clients ─┬─ stdio ─▶ mindleak-mcp ─▶ mindleak-core ─▶ user state/<repo-id>/graph.db
             └─ stdio ─▶ lodestar-mcp ─▶ lodestar-core ─▶ user state/<repo-id>/spec.db

Optional extension sensors ─▶ mindleak-mcp
Configured OpenAI-compatible endpoint (optional) ◀─ mindleak-core / lodestar-core
```

## Crates

### `mindleak-model` (library)

The shared optional-model contract used by both planes: stable failure reasons
(`unreachable`, `timeout`, `bad_json`, `misconfigured`), model-versus-fallback
provenance, on-demand health results, and typed `ureq` failure classification
(ADR-0079). It makes no requests itself. MindLeak and Lodestar retain their own
clients, prompts, budgets, cancellation, and persistence behavior while sharing
one vocabulary that cannot drift between their MCP results.

### `mindleak-storage` (library)

Shared platform-independent repository identity, user-local database resolution,
legacy online migration, backup, and integrity verification (ADR-0013/0038).
Both planes resolve one per-clone id from shared Git config, so linked worktrees
share state without sharing files, indexes, or branches. Reset and export remain
plane-specific operations.

The `repository` module is split by responsibility: `mod.rs` holds the data
types (`DatabaseKind`, `DatabaseOrigin`, `StorageStatus`, `ResolvedDatabase`) and
public re-exports; `resolve` picks the database and state root; `identity` owns
the per-clone repository id and its marker bootstrap; `migrate` guards the
one-shot legacy migration lock; `platform` resolves the OS state root;
`worktree` lists sibling checkouts; and `fs` holds the git-process and
filesystem helpers.

### `mindleak-session` (library)

The shared ADR-0030 request identity contract. It validates client-minted
128-bit session ids, derives the same restart-stable opaque agent id in both
planes, keeps only process-local registrations, and recognizes the narrow legacy
owner shapes eligible for audited claim recovery. Raw session tokens are never
persisted or logged.

### `mindleak-core` (library)

The engine. Modules:

| Module | Responsibility |
|---|---|
| [`config.rs`](../crates/mindleak-core/src/config.rs) | Strict, layered startup configuration for bounded per-project decay policy (ADR-0014). |
| [`model.rs`](../crates/mindleak-core/src/model.rs) | `Node`, `Edge`, `NodeType`, `RelationType`, per-relation half-lives. |
| [`schema.sql`](../crates/mindleak-core/src/schema.sql) | SQLite tables, indexes, FTS5 virtual table + sync triggers. |
| [`db.rs`](../crates/mindleak-core/src/db.rs) | Connection setup (WAL, FKs), migrations, and the `effective_weight()` scalar SQL function. |
| [`decay.rs`](../crates/mindleak-core/src/decay.rs) | The half-life decay formula and prune threshold. |
| [`graph/`](../crates/mindleak-core/src/graph/mod.rs) | `GraphStore`: shared `types`, atomic `writes`, decay-aware `query/` (`lookup` node/FTS resolution, `traversal` bounded walks and impact radius, `agents` roster/attention/overlap), derived `signal/` (`mod` evidence and weighted edges, `promotion` what has earned consolidation, `prune` reaping faded signal), conformance `evidence`, and `lifecycle` operations. |
| [`ingest/`](../crates/mindleak-core/src/ingest/mod.rs) | Zero-token deterministic extractors: `execution`, `tool_invocation` (ADR-0127: a passively captured agent tool call, classified against a committed shell-hygiene pattern list), `git`, `ast`, `structure/{imports,hierarchy}` (JS/TS imports and type hierarchy), `javascript/` (the JS/TS lexer plus `nav`, `scope`, `callable`, `binding`, `shadowing`), and `manifest` (direct package dependencies). |
| [`consolidate.rs`](../crates/mindleak-core/src/consolidate.rs) | Optional OpenAI-compatible consolidation client and worker. |
| [`embed.rs`](../crates/mindleak-core/src/embed.rs) | Optional semantic-recall embedding index (ADR-0008): configured `/v1/embeddings` client, derived `embeddings` table, cosine recall. Off the zero-token write path. |
| [`net.rs`](../crates/mindleak-core/src/net.rs) | Network resilience for optional HTTP (ADR-0010): timeouts, bounded retry with backoff, per-endpoint circuit breaker. |
| [`telemetry/`](../crates/mindleak-core/src/telemetry/mod.rs) | Observability (ADR-0010): durable `telemetry_events` audit trail, metrics snapshot, stderr-only `tracing` init. |
| [`lib.rs`](../crates/mindleak-core/src/lib.rs) | `MindLeak` facade wiring; behavior is grouped under `facade/`: `ingestion`, `query`, `observability`, `lifecycle`, and `consolidation`. |

### `mindleak-mcp` (binary)

A minimal MCP stdio server (newline-delimited JSON-RPC 2.0). Handles
`initialize`, `tools/list`, `tools/call`, `ping`, `shutdown`. Schemas and
handlers live under [`tools/`](../crates/mindleak-mcp/src/tools/mod.rs), grouped
by graph, ingestion, lifecycle, consolidation, embeddings, and telemetry; the
root retains the single timed telemetry wrapper around dispatch.

### `lodestar-core` (library) — the Intent Plane

The **durable** counterpart to the decay graph (ADR-0004): a separate crate and
store so the zero-token decay engine stays uncontaminated. Modules: `model`
(goals/tasks/knowledge), `design` (design items and materialization plans),
`policy` (immutable pack schema/digest validation and the five-clause Common
Core), `controls/` (typed enforcement mechanisms and their ceilings, ADR-0034;
`mod` carries the control kinds, powers and the ceiling rule, `ratchet` the
reviewed baselines and the direction a measure may move from one),
`amendment` and `waiver` (changing adopted policy, and bounded exceptions to it,
ADR-0039), `scope` (the one matcher both clauses and waivers use, so the two
cannot disagree about how far a scope reaches), `fleet` (staleness and
divergence derived from declared session context), `stalls` (why work is not
moving — a pure function over the board), `discovery`, `schema.sql` +
`indexes.sql`, `db` (+ a knowledge `effective_weight` scalar), `decay`
(long-horizon revalidation), `store` (`LodestarStore`: the `goals` and goal↔code
seam, `coordination` task/handoff/conformance ledger, transactional
`policy_packs` proposal/disposition/provenance ledger, reviewed `design/`
materialization plus validation, `amendments` and `waivers`, learned
`knowledge`, and `lifecycle` operations), `llm` (optional OpenAI-compatible model
client), `telemetry` (best-effort local record of each model call this crate
makes — operation, outcome, duration, and token usage when the endpoint
reports it — read back through the `model_telemetry` tool; mirrors
`mindleak-core::telemetry`'s shape but owns a separate table, since a model
call is not a coordination event), `embed` (optional semantic index over
knowledge — a deliberate copy of
`mindleak-core::embed`, because ADR-0004 keeps that crate a dev-dependency only,
so the Intent Plane cannot reach it at runtime), and
`lib` (the `Lodestar` facade wiring). `store/design/` is split by
responsibility: `mod` (register and read), `decision` (the guarded accept/reject
transition and repairing one after the fact), `action` (the append-only,
attributed defer/resume/reject/retire audit), `retirement` (retire and
supersede), `promotion` (promotion into work and its immutable materialization
revisions), and `links` (the current projection of what an item is linked to).
`store/policy_packs/` is split by responsibility: `mod` (pack registration and
retrieval), `proposal` (proposal creation), `query` (proposal and disposition
reads), `review` (attributed adoption/rejection), `provenance` (upstream source
tracking), and `conflict` (pre-adoption conflict detection).
`store/coordination/` is likewise split by responsibility: `mod` (task creation,
shared SQL projections, and row decoding), `claim` (claim/lease CAS, overlap,
scope, and heartbeat), `transitions` (block/reopen/abandon/resolve and progressive
handoffs), `questions` (ask/answer, pause/resume, waits, and thread notes),
`conformance` (audit recording and checked transitions), and `query` (board,
next-task, and existing-work reads). Audited legacy/session recovery remains in
the already-separate `store/claim_transfer.rs` module. For a federated
repository (ADR-0096), `claim`/`renew`/`release`/`recover` and `check_claim_overlap`
route through two injected seams — `FederatedClaimAuthority` and
`FederatedClaimSource` — instead of deciding locally; `None` (the default) is
`CoordinationMode::Local`, unchanged. The crate itself stays local and
network-free either way: only whichever binary composes it with a live,
authenticated `ackplane-client` gives either seam a real implementation.
Facade behavior is grouped under
`facade/`: `constitution`, `executive`, `design`, `design_materialization`,
`conformance/`, `controls`, `amendments`, `waivers`, `advice`, `fleet`,
`evidence`, and `knowledge`. `conformance/` is itself split by responsibility:
`mod` (the `complete_task` / `check_conformance` surface), `clauses` (which
constitutional clauses govern a check), `verdict` (bounded evidence to a
verdict), `token` (what a check was issued against), and `evidence` (shape and
window validation of a submitted bundle).
Each design materialization writes an immutable plan revision; task/goal link
tables are the current projection and can be repaired without deleting history.

**Boot order is load-bearing.** `db::configure` applies `schema.sql`, then
migrations, then `indexes.sql`. Indexes live in their own file because an index
over a column a migration adds cannot be created before that migration runs: on
an existing database `CREATE TABLE IF NOT EXISTS` is a no-op, so the whole batch
fails and the migration never runs. Keeping the phases ordered makes that
structural rather than something each migration author must remember.

**Repository topology (ADR-0038).** Every concurrent workstream uses its own Git
worktree and branch. `mindleak.repositoryId` lives in shared local Git config and
selects one platform-local `repositories/<id>/` directory for both SQLite files.
Lodestar coordinates claims and proof; MindLeak shares learned context; only a
protected pull-request merge advances `main`. `storage_status` exposes the exact
id, path, origin, and legacy migration result on each plane.

The learned-knowledge loop is wired end to end (ADR-0022): `knowledge`'s
`promote_signals` bridge feeds MindLeak proven-signal candidates through the
existing count+span consolidation gate (deterministic, model-optional), and
`conformance` consults `active_knowledge` on every check — a changed node that
intersects a proven regularity attaches an **advisory** finding and may nudge an
otherwise-`Aligned` verdict to `NeedsHuman`, but can never emit `Violation` (only
the Constitution hard-fails). The read path stays deterministic — no LLM.

Agent memory crosses into that loop explicitly (ADR-0081), never through an
editor-private file watcher. The canonical skill classifies client memory while
it still has context, then calls `record_knowledge` with a logical
`/memories/repo/...` or `/memories/session/...` source and node/goal provenance.
`knowledge_sources` keeps one current lesson per source: repeats reconfirm,
edits supersede, and `active_knowledge(source_ref=...)` resolves the current row.
Global user preferences and scratch notes remain outside the repository ledger.

**Evidence is the proof-of-work — the load-bearing guarantee.** Completion is not a
status an agent can assert: `complete_task` consumes only a bounded,
provenance-bearing evidence bundle (`evidence_for`, from the Memory plane) that
`check_conformance` scores against the goal's code bindings, writing a durable,
resolvable record to an append-only `conformance_history` (ADR-0009/0025). In a
multi-agent fleet this chain is the **only** trustworthy proof that the agents did
the sanctioned work — every other signal (an agent's summary, a green check, a PR
body) is narration an agent can fabricate. It is bounded by the live claim,
attributed to the acting agent (ADR-0030), and anchored to real executions and
commits in the graph. `export_evidence` (ADR-0031) renders that chain as a
committed, verifiable artifact so the proof leaves the ledger for human review, a
CI conformance gate (`scripts/conformance-gate.mjs`), and audit — the durable
counterweight to decay: episodes fade, but the record of what conformed survives.

ADR-0026's representation and immutable-pack layers are implemented without a
parallel policy source: `Goal` remains the constitutional clause, while
`constitution_versions` freezes attributed snapshots and `policy_packs` copies
reviewed clauses into local goals with source provenance. Pack rejections remain
durable; conflicts require review; upstream versions cannot mutate local law.
Typed controls, ratchets, amendments, and bounded waivers have since landed on
top of that: a **control** declares the enforcement power a clause actually has
and the ceiling it implies; a **ratchet** binds a metric that must not regress to
a clause and refuses to set its own baseline (ADR-0037); an **amendment** is the
only way to change adopted policy, carrying every active clause forward so the
diff shows what actually changed; and a **waiver** is the reviewable form of
`--no-verify` — scoped, attributed, and always expiring, because an exception
that never ends is the policy (ADR-0039). See
[`SPEC-CONSTITUTION.md`](SPEC-CONSTITUTION.md).

**Knowing why nothing is moving is a first-class read.** `stalls` reports lapsed
leases, work awaiting a human or a peer agent, deadlocked waits, blocks behind
something no agent will advance, and paused work. It is read-only, evidence-free,
and deliberately threshold-free: it reports how long a stall has been true and
leaves "is that too long?" to a person, because a staleness threshold invented in
the engine would become policy nobody agreed to.

[ADR-0024](adr/0024-preflight-overlap-detection.md) adds the coordination layer
above the compare-and-swap claim. Lodestar stores optional claim path globs and
opaque symbol ids in `task_scopes`; its read-only `check_overlap` returns live
scope intersections, each graded from the branches the two sessions declared
(ADR-0035 heuristic 4) as a same-branch collision, cross-branch merge risk, or
undeclared. MindLeak's same-named query derives other agents' direct or
mutation-linked footprint after decay filtering. The caller combines those two
results by node id: no shared tables, transactions, or plane dependency. The VS
Code allocator performs both reads before claiming, displays an overridable
warning, and renders persisted scope in the Work view. The flow is advisory
(never a lock, per ADR-0015) and complements ADR-0018's physical integration
discipline.

### `lodestar-mcp` (binary)

A second MCP stdio server exposing the Intent Plane tools for constitution,
tasks, conformance, and knowledge. It uses the same newline-delimited JSON-RPC
as `mindleak-mcp`; schemas and handlers are grouped under `tools/` by
constitution, executive, conformance, knowledge, lifecycle, controls,
amendments, waivers, design, design materialization, evidence, and fleet
responsibility. See [`USAGE.md`](USAGE.md) for the workflows those verbs
compose into — the tool tables in [`TOOLS.md`](TOOLS.md) describe each verb,
not the order to call them in.

### `mindleak-coordinator` (binary)

A third, thin MCP stdio server (ADR-0097): the one agent-facing entry point
that composes `mindleak-mcp` and `lodestar-mcp` rather than adding a third
source of graph or intent truth. `main.rs` spawns both as real child
processes (`MINDLEAK_MCP_BIN`/`LODESTAR_MCP_BIN`, or a sibling binary next to
it — the same env-var convention `scripts/canonical-push.mjs` already uses)
and performs their `initialize` handshake before serving anything itself.
`child.rs`'s `ChildClient<R, W>` is a minimal newline-delimited JSON-RPC
client generic over its transport, so it is unit-tested against injected
in-memory streams (mirroring how `clients/node/mindleak-client`'s
`McpConnection` is tested) rather than a real subprocess; one end-to-end test
does spawn the real binaries, but against a throwaway `git init`-ed directory
under the OS temp dir so it resolves an isolated `repository_id` instead of
writing into whatever repository the test happens to run inside. `tools.rs`
composes decision 2 (`coordinator_open_session`: opens both planes with the
same declared context and verifies they resolve the same `agent_id` and
`repository_id`, naming whichever plane failed rather than presenting a
partial open as complete) and decision 3 (`coordinator_preflight`: runs
MindLeak's `check_overlap`, Lodestar's `task_query(view="overlap")`, and
Lodestar's `advise` and merges them with per-plane provenance — the read a
write should already have made). `server.rs` is the coordinator's own stdio
front, mirroring `mindleak-mcp`/`lodestar-mcp`'s transport exactly. This is
the first slice of ADR-0097; decisions 4-8 (client-side Git observation,
goal-less scope reservations, governance-bootstrap helpers, memory-source
reconciliation, and the usage retrospective) remain future work under the
same task.

### `ackplane-core` (library)

The repository side of the Ackplane federation boundary (ADR-0082). What a
repository must settle before it coordinates at all is which arbiter owns its
claims. A repository declares its mode via the `mindleak.coordinationMode`
git config key (repository-scoped, ADR-0082 decision 3) and/or the
`MINDLEAK_COORDINATION_MODE` environment variable (a process-local override);
declaring both is fine when they agree, and refused when they do not, so two
processes for the same repository can never silently settle on two different
arbiters. Both planes resolve it once at startup rather than per call. An
unrecognised value, or a `federated` repository this build has no client to
reach, is refused rather than quietly arbitrated locally: that downgrade would
be the second arbiter ADR-0045 forbids.

`compiled_federation_readiness` distinguishes *why* federation is unusable —
`NoClient` (the `federation-client` cargo feature is off, so
`ackplane-client` is not linked), `ArbiterUnreachable` (the feature is on but
`MINDLEAK_ACKPLANE_ENDPOINT` is unset or the deployment did not answer) — so
the refusal a repository sees names its actual remedy instead of asserting a
rebuild that would not help.

### `ackplane-client` (library)

The repository-side gRPC client for `ClaimDelegationService`
(`task:727ae37b4f5a`, ADR-0096): `delegate_claim`, `renew_claim`,
`release_claim`, `recover_claim`, and a bare `probe_reachable` used only for
mode resolution. It depends on `ackplane-protocol` alone — never
`mindleak-core` or `lodestar-core` (ADR-0082 clause 1's boundary runs through
this crate too: the client sits on the repository side and must not smuggle a
plane dependency back). `ackplane-core` links it only behind the
`federation-client` cargo feature, off by default, so `mindleak-mcp` and
`lodestar-mcp` remain network- and async-runtime-free in their default,
standalone build (ADR-0094 decision 1).

What this crate does not do: decide *when* to call itself, or what to do with
the answer. `lodestar-mcp`'s `federation.rs` (built only behind
`federation-client`) is the one place both `lodestar-core`'s
`FederatedClaimAuthority` trait and this crate's `ClaimClient` are both
reachable: it resolves this repository's federated identity (endpoint,
tenant, repository, node, signing key) from explicit configuration, signs
every request with `ackplane-client::authenticate`, bridges each synchronous
call to this crate's async methods with a fresh current-thread runtime
(mirroring `compiled_federation_readiness`), and projects a grant into the
local task cache through `lodestar-core`'s cache-projection APIs. A rejection
or transport failure never falls back to local arbitration — it resolves to
the same "lost the CAS" outcome a local claim would (rejection) or an
`Err` naming the transport failure (unreachable), and either way the local
row is untouched. The residual gap this closed is narrowed in
[`gaps.d/no-claim-is-arbitrated-through-ackplane.md`](../gaps.d/no-claim-is-arbitrated-through-ackplane.md).
The node signing key is sourced from the OS credential facility (Windows
Credential Manager, macOS Keychain, or Linux Secret Service, via the
`keyring` crate) by default, per ADR-0085 decision 2 and ADR-0100 decision 5;
an explicit environment variable remains available as a documented,
non-hardened override for tests and constrained deployments.

`NodeSyncConnection` (`ackplane-client::node_sync`) is the first genuinely
reusable client side of ADR-0116's supervisor runtime: it opens the
bidirectional `Synchronize` stream, sends `Hello`, and completes the
enrolled-key connection challenge -- the same handshake
`ackplane-server::service::handshake` implements and tests server-side --
returning a live authenticated frame sender/receiver only after `HelloAccepted`
and `FlowControl` are observed. Signing goes through the existing
`ClaimSigner` trait, never a raw key inline in this module. A wrong signature,
an unknown `signing_key_id`, or a revoked key surface as
`ClientError::ConnectionRefused` carrying the server's own typed
`RejectionReason`, not a bare stream failure. The connection-challenge byte
layout itself (`ConnectionChallengeBinding`, `connection_challenge_bytes`)
moved to `ackplane_protocol::connection_challenge_auth` so the two sides can
never drift into incompatible encodings of the same signed fields;
`ackplane_server::enrollment` re-exports it unchanged for its existing
callers. `exchange_supervisor_frame` sends one supervisor frame --
registration, session, or heartbeat -- and awaits its `SupervisorFrameReceipt`
without hand-rolling the wait: flow control and notices are stepped over as
server housekeeping, a `Rejection` surfaces as a typed
`ClientError::FrameRefused` (carrying the server's own `RejectionReason`,
distinct from `ConnectionRefused` because the connection itself is healthy and
reconnecting fixes nothing), and any other frame is a protocol violation
reported as `ClientError::UnexpectedFrame`. `exchange_event_batch` gives the
same typed treatment to a published event batch, returning its
`BatchReceipt`. `next_directive` drains the connection's already-buffered
`AgentDirective`s non-blockingly -- the server delivers a session's pending
directives ahead of the receipt for the frame that addressed them, so a
caller holding that receipt has necessarily already read every directive
delivered with it -- and `submit_directive_receipt` answers one back over the
same `exchange_supervisor_frame` round trip. Reconnect reconciliation is a
separate concern the daemon owns, not this connection: see
`ackplane-supervisor` below and [ADR-0135](adr/0135-a-directive-receipt-survives-a-dropped-connection.md).

### `ackplane-supervisor` (library and binary)

The local durable directive inbox and outbox for one enrolled supervisor and worker session (ADR-0116). It owns a caller-provided SQLite database rather than either repository-local plane database. The inbox binds itself to one tenant, repository, node, supervisor, and agent session; verifies that an incoming directive targets that identity and names an advertised capability; rejects expired or out-of-sequence delivery; and persists accepted, capability-refused, and expired receipts before returning them. Replaying the same directive id and payload digest returns the original stored receipt, while a changed digest under the same id is refused without overwriting evidence.

The outbox persists encoded `NodeFrame`s before a future sender may transmit them. It assigns a local positive contiguous sequence through a persisted high-water mark, replays an identical frame idempotently, refuses a changed frame at an existing sequence, returns pending frames in sequence order under a bounded limit, and prunes only frames at or below a caller-supplied acknowledged sequence. The inbox and outbox share the same durable supervisor identity/session binding so a different node or worker session cannot reopen the queue database.

It does not open a network listener. `reconcile.rs` is ADR-0116 decision 7's reconnect comparison: a pure function over the outbox's durable positions and the position the server reports, returning `UpToDate`, `Resend`, or `IncompleteEvidence`. The two directions are deliberately asymmetric — a server behind the outbox is ordinary catch-up, while a server *ahead* of it means local durable state is behind reality, so frames exist that this supervisor published and can no longer describe. That is reported and blocks resumption rather than being resent through, because resending from the server's position would look identical to a clean resume while skipping exactly those frames (decision 3). The acknowledged position is derived from surviving frames while the enqueue high-water mark stays persisted; that pairing is what keeps both correct.

`daemon/mod.rs`, `daemon/frames.rs`, and `main.rs` are the runnable `ackplane-supervisor` binary (ADR-0116: "an enrolled supervisor is the only Industrial runtime endpoint"). `frames.rs` holds pure, I/O-free wire-frame builders (registration/session/heartbeat) split out so `mod.rs`'s `serve_once` reads as control flow. It resolves configuration from the same `MINDLEAK_ACKPLANE_*` variables `register-me` and the federated claim path already use, plus `ACKPLANE_SUPERVISOR_ID`/`_STATE_DIR`/`_HEARTBEAT_SECONDS`; refuses at startup naming every missing variable at once; then connects, registers, opens a session, heartbeats, and durably receipts each delivered directive. With no `WorkerAdapter` wired in it declares exactly one capability, `Notify` — a notification is complete once durably recorded, so accepting one is truthful — and declares none of the worker-driving capabilities, so Ackplane refuses to enqueue work it cannot do and an `Accepted` receipt for unperformed work is unreachable rather than merely unlikely (decision 10). A directive receipt is enqueued into the durable outbox before transmission and acknowledged only once Ackplane's own frame receipt confirms it, so one lost to a dropped connection is resent from the outbox on the next connect rather than depending on server-side redelivery; a non-retryable server refusal is dropped rather than resent forever. `serve_once` deliberately does **not** call `reconcile` on connect: `HelloAccepted.accepted_position` only echoes the `last_accepted_position` the client itself just sent, so comparing it against the outbox's own position could only ever answer `UpToDate` and an earlier revision that made this call was a dishonest guard, not a working one (recorded in `gaps.d/ackplane-never-reports-its-own-supervisor-position.md`; detecting a server genuinely ahead needs a wire-protocol change Ackplane does not yet make). `reconcile` itself stays correct and unit-tested for when that server-reported position exists.

`worker_adapter.rs` is ADR-0116 decisions 5 and 9's small, runtime-neutral `WorkerAdapter` trait: `start`/`observe`/`terminate` are mandatory, and `checkpoint`/`pause`/`drain` default to a typed `AdapterError::Unsupported` refusal an adapter overrides only when it can genuinely enforce the control, never approximated. `ProcessWorkerAdapter` is the reference implementation: it owns a map of local child processes, starts a declared command and argument vector (never a shell string, per decision 5's "cannot execute arbitrary shell strings"), and every method refuses to act on a `worker_id` it did not itself register with a distinct `UnknownWorker`/`DuplicateWorker` error rather than silently no-op'ing. `observe` maps a live/exited/failed child to `SupervisorWorkerState`; `terminate` kills a still-running child and is idempotent against one that already exited. Because checkpoint, pause, and drain are not honestly enforceable against a generic OS process, `ProcessWorkerAdapter` relies on the trait's default refusal rather than approximate them. Wiring it into the daemon so a directive is genuinely executed is a separate, deliberate change.


### `ackplane-server` (binary)

The service side of the same boundary, and a separate deployable rather than a
mode of either plane (ADR-0082 clause 1) — it depends on no plane crate, and on
`ackplane-core` not at all. A deployment declares its ledger and its durability
profile before it may accept work, and both are refusals rather than defaults.

Without `ACKPLANE_DATABASE_URL` it will not start, because an instance holds no
authoritative local state and cannot accept work without the ledger (ADR-0086
clause 1); starting anyway would turn a misconfiguration into a failure on
whichever request happened to arrive first. It binds loopback by default and
prints a banner naming its profile (ADR-0088 clause 6). A `quorum_durable`
claim is refused unless `ACKPLANE_SYNCHRONOUS_STANDBYS` names the failure domain
it rests on, because a durability claim nothing backs is exactly the
asynchronously replicated acknowledgement ADR-0086 clause 12 will not label as
zero-loss. The database URL carries a password, so it is absent from the banner
and redacted in a hand-written `Debug`.

TLS is optional only for loopback development listeners. A deployment that sets
`ACKPLANE_LISTEN` outside `127.0.0.1` or `::1` must also set both
`ACKPLANE_TLS_CERTIFICATE_PATH` and `ACKPLANE_TLS_KEY_PATH` to PEM files; the
server refuses partial or absent TLS material before accepting work (ADR-0083
clause 8).

The server exposes `NodeSyncService.Synchronize` from ADR-0083 and the initial
enrollment path from ADR-0085. The synchronization stream acknowledges a hello,
translates event batches into ledger appends, returns durable positions in
receipts, and emits typed rejections for malformed or conflicting records.

`DirectiveStore` (`directive_store/`) is the durable, pre-delivery half of
ADR-0107's control channel. It accepts only the closed `AgentDirective` wire
family for an already registered supervisor session, validates the shared
directive-to-capability mapping and domain-separated typed payload digest,
assigns server time and per-target sequence, and retains bounded encoded
directive envelopes before a live sender delivers them. `DirectiveReceipt` records append separately only when their
tenant, repository, node, session, sequence, and payload digest bind exactly
to the stored directive; identical retries replay their original record. `service/directive_delivery.rs` (ADR-0116 slice 3) is that sender: it attaches
undelivered directives ahead of the `SupervisorSession` frame's own receipt,
so that receipt becomes a delivery barrier a reconnecting supervisor also
redelivers through -- at-least-once, and honest that a frame put on a stream
is not evidence a supervisor acted; only the directive's own later receipt is.

Authenticated `NodeSync` streams ingest `DirectiveReceipt` frames through that same ledger only after their tenant, repository, and node match the completed connection challenge. Ackplane returns a typed `SupervisorFrameReceipt` for a durable write or exact replay, resolving the supervisor only from the receipt's scoped session; unknown or cross-scope directives receive a generic refusal rather than a directive-existence disclosure. Outbound delivery (above) and the Bridge's own issuance route (`WorkCommandStore`/`work_command_api/` below) close the two ends this ingress path originally left open.

`ContextPacketCompiler` (`context_packet_compiler.rs`) is ADR-0114's pure deterministic envelope compiler. It validates every candidate before selection, reserves mandatory governance, task, and evidence context under the requested token budget, and refuses when that mandatory set cannot fit. Optional candidates sort by relevance then stable identifier; each is included whole or retained as a typed budget exclusion. It returns the existing validated `ContextPacket` protocol contract and makes no model call, persistent write, authenticated transport, or action-authority decision; later service slices own those boundaries.

Authenticated `NodeSync` streams also accept the closed `WorkTaskCreate` frame for one native Industrial Work record. The frame carries only a node-scoped creation id and bounded task content; the completed connection challenge supplies tenant, repository, and publisher identity. Ackplane derives an opaque Work id from that authenticated identity and creation id, writes the current task projection and initial history event transactionally, and returns a typed `WorkTaskReceipt`. An exact retry returns the original Work id with `idempotent_replay`; changed content under the same creation id receives a non-retryable conflict. This is native Ackplane Work publication, not a Local Lodestar import: the Bridge remains a bounded read-only projection and exposes no Work mutation route.

`WorkCommandStore` (`work_command_store/`) is ADR-0125's durable Work command/receipt persistence primitive. Its authoritative command-service caller must validate authorization first; the store records the closed command vocabulary, schema version, tenant/repository and optional Work-task scope, principal/delegation/policy references, bounded rationale, expected task version, confirmation and expiry references, idempotency key, canonical payload digest, and immutable receipt outcomes. Exact command or receipt retries return their original record; reusing an identity with changed content is refused. A confirmed `CreateWork`/`RouteWork`/`ReleaseLease`/`AnswerWait`/`SubmitReview` executes its Work/Claim mutation and receipt atomically in one transaction (`execute::execute_confirmed`); a confirmed `Assign`/`Steer`/`Pause`/`Resume`/`Drain` instead issues a matching ADR-0107 directive to the enrolled supervisor session its payload names, on that same transaction (`execute::supervisor_directives`, sharing `directive_store::enqueue_in_transaction` rather than a second connection) -- the receipt records `pending_delivery` or a typed refusal, never that the worker already acted. Only the addressed supervisor's own later `applied`/`refused`/`failed`/`expired` directive receipt, applied through `WorkCommandStore::apply_directive_receipt`, appends the corresponding `work_task_history` event and, for `applied`, moves the task's projected state; `accepted` never does. The module and its authorization types (`WorkCommandService`, `WorkCommandAuthorization`, `WorkCommandServiceOutcome`, the per-kind payload structs) are now `pub`, so the Bridge route below is the one caller ADR-0125 decision 11 requires -- no other module reaches `WorkCommandStore` directly.

`WorkCommandService` (`work_command_store/service.rs`) is the ledger's only production writer. It evaluates an authentication result before the first command lookup: the loopback developer profile returns `authorization_unavailable`, while missing, forged, tenant/repository-out-of-scope, command, policy, or delegation mismatches return typed refusals without disclosing a target. A verified in-scope request records only the immutable command and a `pending_confirmation` receipt; `confirm` re-checks identity and the exact payload digest before executing.

The Bridge's `work_command_api/` (`ackplane-bridge`) is ADR-0125's first command route: `POST /api/v1/repositories/:repository_id/work/commands` (submit) and `POST /api/v1/repositories/:repository_id/work/commands/:command_id/confirm` (confirm), covering all ten command kinds through one JSON envelope tagged by `kind`. Split across three files -- `mod.rs` (state, routing, handlers), `payload.rs` (wire payload shapes and their conversion), `response.rs` (the typed outcome vocabulary) -- to stay under this repository's module-length control. It calls `WorkCommandService` directly -- never `WorkStore`/`ClaimStore` -- and, since the Bridge's loopback developer profile is not a verified principal, every request resolves to `WorkCommandAuthorization::LoopbackDevelopment` and returns the same typed `authorization_unavailable` outcome the read-only capability list already advertised. This proves the full request/response contract (envelope shape, per-kind payloads, typed outcomes, `CreateWork`'s task-id/expected-task-version exclusion) against a real `WorkCommandService`, without granting the loopback profile any authority ADR-0125 decision 2 withholds from it. `ackplane-workctl` (`crates/ackplane-workctl`) is a small standalone CLI binary that calls this same versioned API over plain HTTP -- a scriptable, non-browser caller of the identical route and outcome contract, not a second way to reach the command service.

The Bridge Work read response also reports ADR-0125's closed Work-command vocabulary. Under the loopback developer profile, every command is `authorization_unavailable`: the tenant token is not a verified principal and no authorization verifier or policy basis exists. The Work page renders those named controls disabled with the same accessible reason; the command route above returns the identical outcome when actually invoked.

`work_command_vocabulary.rs` declares the ten Work command operation names (ADR-0125 decision 3: "gRPC, HTTP, and future clients cannot grow different meanings of scope") as one canonical public `WORK_COMMAND_OPERATIONS` constant, in `WorkCommandKind`'s stable wire order. Both `WorkCommandKind::operation_name()` and the Bridge's `command_capabilities()` now derive from this single constant, replacing a Bridge-local hardcoded duplicate that could otherwise drift from the store's own vocabulary. It is deliberately data-only and exposes no store, service, or mutation capability of its own.

`HumanDecisionStore` (`human_decision_store/`, migration `0054_human_decision_requests.sql`) is ADR-0115 item 5's durable escalation namespace: a request records its proposing principal, proposed action, target, reason, context-packet and evidence digests, alternatives, safe behavior, and an optional related delegation, and its append-only event stream advances a per-tenant/repository position guarded by an idempotency key and canonical payload digest, exactly like the other stores in this section. It is intentionally server-internal -- it exposes no command surface itself, and it never treats a session label, model score, or agent assertion as a decision. The Bridge's `human_decision_api.rs` (`ackplane-bridge`) is its first read surface: a `/decisions` page and paginated `/api/v1/repositories/:repository_id/decisions[/:decision_id]` routes render the pending-decision queue, so an escalation is now visible in the Bridge rather than hidden in agent logs. This module is deliberately read-only; the approve/refuse intervention item 7 describes is a separate later command path with its own rationale and receipt, and a browser `GET` must never become the thing that resolves an escalation.

`NodeEnrollmentService` persists pending requests, an append-only authority
transition history, approved public-key bindings, single-use short-lived
challenges, and immutable enrollment receipts. A node may submit a request, but
it cannot approve itself: an independently authenticated administrator must
approve the exact fingerprint before the service issues a challenge. Ackplane
verifies the node's Ed25519 proof over a domain-separated, bound challenge and
atomically consumes it while recording `activating`. Key rotation remains
explicitly unavailable until the continuity proof required by ADR-0085 is
implemented.

`register-me` (`src/bin/register-me.rs`) drives that ceremony from the command
line as three subcommands — `request`, `approve`, `activate` — mirroring the
real actors: a node runs `request`/`activate` unattended; `approve` is a
documented local-dev database shortcut standing in for the administrative
approval RPC/UI that does not exist yet. `activate` proves possession, opens
one real `NodeSync` stream, and sends a signed heartbeat event using the
`signing_key_id` `EnrollmentActivationResult` returns directly.

`KnowledgeService` (`knowledge_store.rs`/`knowledge_service.rs`) is the first
slice of Ackplane's PostgreSQL-backed knowledge domain (ADR-0106 decision 3;
distinct from `clients/node/mindleak-client`'s same-named service, which
wraps the *local* planes' own knowledge tools instead): `RecordKnowledge`,
`RecallKnowledge`, `RetireKnowledge`. Effective weight is the same decay
formula as `mindleak-core::decay::effective_weight`
(`W_eff = W_base * 2^(-Δt_hours / half_life)`), expressed as a Postgres `CASE`
expression and computed at read time — never stored — matching this
repository's standing decay invariant on Ackplane's side too. A recall with a
query embedding ranks by pgvector's own `<=>` cosine-distance operator
entirely inside Postgres; without one, entries recall by effective weight
(recency, decay-adjusted) instead, the same graceful degradation ADR-0080
established for the local planes. Embeddings are fixed at 768 dimensions
(`nomic-embed-text`, this repository's shared default embedder) for this
first slice; a second model at a different dimension needs its own column or
table, not a redesign. Like `ClaimDelegationService`, these RPCs authenticate
every mutating request with a signed `KnowledgeAuthentication` (ADR-0108):
its own domain separator (`knowledge_auth::KNOWLEDGE_DOMAIN`), its own
`KnowledgeOperation` enum binding each RPC's operation-specific fields into
the signed bytes, and its own `knowledge_authentication_nonces` table —
mirrored, not shared with claims', so the two domains' replay protection and
signed-byte encodings can never collide or drift into each other.

A recorded statement's `lifecycle_state` (`KnowledgeLifecycleState`,
`knowledge_store/activation.rs`) starts as `Candidate` and is never implicit
or cosmetic (ADR-0113 decision 1): `recall`/`active_page` only ever return
`Active` rows, so an unreviewed candidate is invisible to both the gRPC
recall path and the Bridge's read-only page until a separate, authorized
`activate` call promotes it. `activate` is a compare-and-swap (mirroring
`DesignStore::record_decision`'s own CAS) guarded on `lifecycle_state =
Candidate AND retired_at IS NULL`; a failed guard is diagnosed precisely
(`UnknownKnowledge`, `AlreadyActive`, `Retired`) rather than reported as one
generic conflict, and an empty `authorized_by` is refused before the CAS
ever runs. Every accepted transition appends one immutable row to
`knowledge_activations` (ADR-0113 decision 7) — the authorization basis, an
optional reason, and when — never updated or deleted once written, the same
append-only contract `knowledge_reconfirmations` already holds. `retire` and
`reconfirm` are unchanged and still operate independently of
`lifecycle_state` (a never-activated candidate remains retirable; a
candidate remains reconfirmable, so corroboration gathered before review is
not lost). Rows recorded before this decision landed backfill as `Active` on
migration, so already-established guidance does not vanish. Wiring
`activate` into an authenticated gRPC RPC or a Bridge review surface is
deferred to a later decision, matching this repository's established
read-model-first rollout order.

An active statement can also be superseded (`KnowledgeStore::supersede`,
ADR-0113 decisions 1 and 7): a third `lifecycle_state`, `Superseded`,
distinct from retirement — the prior row is preserved exactly as it stood
(`retired_at` stays `NULL`) and instead gains a `superseded_by` pointer to
its replacement. `supersede` inserts the replacement directly as `Active`
(the supersession's own authorization basis already satisfies decision 1's
review gate) and marks the prior statement `Superseded`, both inside one
`WITH` statement alongside an append-only `knowledge_supersessions` receipt
naming who authorized the change and — required and non-empty, unlike
`activate`'s optional reason — why the replacement won. The guard
(`lifecycle_state = Active AND retired_at IS NULL`) refuses a still-
`Candidate`, already-`Superseded`, or retired prior statement with the same
precise-diagnosis pattern `activate` uses, plus a distinct
`ConcurrentlyModified` outcome when a read taken after a failed guard still
finds the row `Active` (a genuine race between the failed compare-and-swap
and the diagnostic read, not a case the other outcomes correctly describe).
Separately, `record_evidence_reference` (decision 3) attaches a bounded,
append-only trail of outcome/evidence references — a task, context packet,
validation run, or receipt — to a statement in any lifecycle state, each
tagged with a `polarity` (`Corroborates`/`Contradicts`) recorded as its own
fact rather than folded into one opaque confidence number; a later
contradiction is a new reference, never an edit to an earlier corroborating
one. `evidence_references` returns them recency-first, hard-bounded at 100
rows regardless of the caller's requested limit.

Stale or overdue-for-revalidation knowledge is surfaced through a fourth,
read-only view (ADR-0113 decision 4): `revalidation_queue`/`revalidation_entry`
(`knowledge_store/revalidation.rs`) classify every `Active` statement as
`current`, `approaching_expiry`, `contradicted`, or
`overdue_for_revalidation` from three signals computed at query time and
never stored -- effective weight (the same decay expression above), whether
any `knowledge_evidence_references` row for it carries `Contradicts`
polarity, and whether an optional, nullable `revalidate_after_hours` policy
column (migration `0036`, no authoring surface exists yet) has elapsed since
`confirmed_at`. Precedence is fixed: a contradiction outranks an overdue
policy rule, which outranks the generic half-life curve, because a
contradiction is an evidence-backed refutation independent of elapsed time
and an overdue rule is a human-authorized constraint more specific than decay
alone; `effective_weight <= 0.5` is exactly one elapsed half-life. The
classification is expressed twice by design -- a pure Rust function
(`classify`, `#[cfg(test)]`-only, for direct DB-free testing of the
precedence rule) and an equivalent Postgres `CASE` expression
(`CLASSIFICATION_SQL`) that actually runs in `revalidation_queue`/
`revalidation_entry`, the same intentional duplication `EFFECTIVE_WEIGHT_SQL`
already establishes in this crate. `revalidation_queue` excludes `current`
records, supports an allow-listed classification filter, and paginates via a
stable `(confirmed_at, knowledge_id)` keyset cursor bounded at 100 rows per
page; `revalidation_entry` looks up one record by id without excluding
`current` (a detail view, not the review queue itself). Bridge exposes this
at `GET /api/v1/repositories/:repository_id/knowledge/revalidation-queue`
(`knowledge_api.rs`) with `classification`/`limit`/paired
`after_confirmed_at_micros`+`after_knowledge_id` query parameters,
tenant/repository-scoped like every other knowledge route. Both this and the
history route carry a record's reach as a bounded `reach_node_ids` preview
beside `reach_count` and a `reach_truncated` flag — the count keeps describing
the whole set, only the array is capped — so the Knowledge page can open a
lesson's reach in the Context Graph rather than only counting it; this task is
deliberately read-only and adds no mutation route, no policy-authoring
surface, and no automatic revalidation trigger.

`ConstitutionService` (`constitution_store/`/`constitution_service.rs`) is a
read-only projection of a repository's own authoritative local Lodestar
constitution (ADR-0106 decision 3): `PublishConstitutionSnapshot` (called by
that repository's own tooling) and `GetActiveConstitution`. Ackplane never
adopts, tailors, rejects, promotes, or waives a clause — those actions stay
local to Lodestar; a publish replaces the tenant/repository's snapshot
wholesale (delete-then-reinsert its clauses inside one transaction), never an
incremental diff, because a constitution version is immutable at the source.
Mirrors `KnowledgeService`'s authentication pattern exactly: its own domain
separator (`constitution_auth::CONSTITUTION_DOMAIN`), its own
`ConstitutionOperation` enum, and its own
`constitution_authentication_nonces` table.

Beside that mutable active snapshot, `PublishConstitutionSnapshot` also
records an immutable, append-only publication history entry (ADR-0121
decision 1: `ConstitutionStore::record_publication`, table
`constitution_publications`, keyed on `(tenant_id, repository_id,
version_id)`). The immutable record is checked first: retrying an identical
publication is an idempotent no-op, but publishing the same `version_id`
under different content is refused before the mutable snapshot ever changes,
so a rejected republish never silently moves the active pointer. Ackplane
still never originates a publication itself (decision 2) — it verifies the
publisher and stores the resulting projection.

A separate, append-only `constitution_proposals` table (ADR-0126, table
`constitution_proposals` in `constitution_store/proposals.rs`) holds
Bridge-originated suggestions for a constitution clause change —
`ConstitutionStore::propose_clause` (idempotent on `(tenant_id,
repository_id, proposal_id)`, refusing a mutated retry the same way
`record_publication` does), `list_proposals`, and `withdraw_proposal`
(gated to the proposal's own author). A proposal carries no authority of
its own: it is never read by `get_active`/`publish`, and adoption is
read-only pattern matching over this table and `constitution_publications`
at Bridge read time, never a status this table's own writer sets — the
same "distribution is not activation" boundary ADR-0082 decision 4 and
ADR-0121 decision 2 already draw, extended to this lighter object.

`DesignStore` (`design_store/`) is a separate Industrial-only authority
(ADR-0121 decision 3), distinct from the Constitution projection above: an
opaque `(tenant_id, repository_id, design_id)` design record carries bounded
title/summary/source_version, a closed-vocabulary `lifecycle_state`
(Proposed/Accepted/Rejected/Deferred/Retired/Superseded/Materialized), and
optional references into the Constitution/Work/Evidence domains, each
enforced by a real composite foreign key against the referenced table in the
same tenant and repository. The Work reference was added by a follow-up
migration once the Work domain's own schema landed (`work_tasks`,
ADR-0120); it was deferred out of the original migration until then.
Creation is a digest-checked idempotent insert (an identical retry no-ops;
the same `design_id` reused with different content is refused) that also
writes the design's first, `Proposed` row into the append-only
`industrial_design_decisions` history table. Every later lifecycle
transition is a separate `record_decision` call appending one more history
row and moving the design's own `lifecycle_state` to match, in one
transaction — nothing in this store enforces which transitions are legal or
who may request one; ADR-0121 decision 3 defers that authorization to a
future typed command. The Bridge's `design_api.rs` now exposes this store
directly (`GET`/`POST /api/v1/repositories/:repository_id/designs`, its
`:design_id` detail, and a `/decisions` route) -- an unauthenticated write
surface today, exactly as unenforced as the store itself, since decision 3's
authorization command remains future work.

`MaterializationStore` (`design_materialization_store.rs`, a separate file
from `design_store/` to stay under the module-length ratchet) is ADR-0121
decision 4: an append-only, idempotency-key-scoped revision history of
materialization decisions against a design, distinct from the design's own
lifecycle-transition history above. A revision records its actor, a
caller-supplied `idempotency_key`, optional rationale, a REQUIRED
constitution_version_id reference (which Constitution publication the
materialization was informed by), bounded optional Lodestar `goal_ids`
(plain strings -- Lodestar goals live in a different plane, so these are not
FK-checked), and a payload digest. Work-task references go through a
junction table (`industrial_design_materialization_work_tasks`) rather than
a bare array column, so each one is a real foreign key like every other
cross-domain reference in this crate. `record_materialization` mirrors
`evidence_store`'s own established idempotency contract exactly: an
identical resubmission (same `idempotency_key`, same every other field)
returns the original revision unchanged; the same `idempotency_key`
resubmitted with any different field is refused as a conflict. A genuinely
new submission (a fresh `idempotency_key`) always gets the next
`revision_number` for that design, even if its content happens to match an
earlier revision -- revisions are never deduplicated by content, only by
idempotency key. The Bridge's `design_api.rs` exposes this too
(`POST .../designs/:design_id/materializations`), sharing the same
unauthenticated-actor caveat as the decision route above.

`TelemetryService` (`telemetry_store.rs`/`telemetry_service.rs`) is Ackplane's
typed operational-telemetry domain (ADR-0105 decision 6): `RecordTelemetry`
(tool/transport/directive/storage/projection observations, each with a bounded
count of small typed measurements) and `ReadTelemetry` (per-name current health
plus bounded time-bucketed series). Health is derived at read time from the
most recent success/error, never a lifetime count — `currently_failing`
compares `last_error_at` against `last_success_at`, so a resolved past error
stops reading as an active fault the moment a later call succeeds, mirroring
mindleak-core's own local `NameMetric`/`currently_failing` logic (ADR-0010)
expressed as a Postgres aggregate instead of a SQLite query. Bucketed series
use `date_bin` and a `ROW_NUMBER() OVER (PARTITION BY kind, name ...)` window
to keep only the most recent `max_points` buckets per series, so an unbounded
read never becomes an unbounded response. Same authentication pattern as
`KnowledgeService`/`ConstitutionService`: its own domain separator
(`telemetry_auth::TELEMETRY_DOMAIN`), its own `TelemetryOperation` enum, and
its own `telemetry_authentication_nonces` table.


A separate axum HTTP server for the Bridge (assurance operations, ADR-0090):
tenant-scoped Fleet views over Ackplane's accepted Postgres state for one
development tenant, resolved from a loopback-only salt file
(`ACKPLANE_BRIDGE_SALT_PATH`). It links `ackplane-server::fleet` directly and
started as a predominantly read-only service. ADR-0111's tenant-scoped, reason-required
stranded-claim recovery was the first Bridge mutation; the loopback developer
profile's own Administration (`administration/`), Design (`design_api.rs`),
Constitution proposal (`propose_clause`/`withdraw_proposal`), and Work command
(`work_command_api/`) routes below have since added their own bounded write
paths, each under its own authorization model -- an adopted policy for
Administration, an author-gated withdrawal for a Constitution proposal, and a
typed `authorization_unavailable` refusal for every Work command until a real
verified-principal authenticator exists (ADR-0125 decision 2). Enrolment and
normal node-signed claim lifecycle (`delegate`/`renew`/`release`) remain
behind Ackplane's typed gRPC service boundaries; the Bridge never reaches
them. Current routes, each 404 on a repository the tenant has not
enrolled rather than leaking a distinguishable error:

`crates/ackplane-bridge/Dockerfile` (ADR-0105 decision 3) packages one binary
onto the same pinned toolchain/runtime pattern `crates/ackplane-server/Dockerfile`
uses, and `docker-compose.yml`'s `bridge` service brings it up alongside
`ackplane` so a developer gets both from one `docker compose up`. Bridge never
gRPC-connects to `ackplane`; every store is a direct Postgres connection to the
same database `ackplane` migrates, so `bridge` only depends on `migrate`
completing, not on `ackplane` itself running. `BridgeConfig::resolve` refuses
any non-loopback listen address until a production authentication verifier
exists (ADR-0094), so the process always binds `127.0.0.1` inside its
container -- but Docker's published-port mechanism forwards host traffic to a
container's real network interface, never to that container's own loopback,
so a port published straight from that bind is unreachable from the host
despite the container reporting healthy (a healthcheck runs inside the same
network namespace, where loopback works regardless). `docker-entrypoint.sh`
resolves this without touching `BridgeConfig`'s validation at all: it starts a
`socat` relay on a second in-container port, listening on every interface and
forwarding to the Bridge process's own loopback bind, and the Compose service
publishes that relay port to the host rather than Bridge's own.

`administration/` is the ADR-0105 decision 6 "Backup / export / reset" parity
row, scoped by ADR-0119's accepted policy rather than a mechanical copy of the
VSIX's `mindleak.backup`/`mindleak.export`/`mindleak.resetMemory` commands.
ADR-0128 recognizes the hardened loopback developer profile itself as the
verified principal ADR-0119 decision 2 requires, so Snapshot is now a real,
receipted capability rather than a permanent refusal -- but only once an
`administration_store::AdministrationPolicy` naming the operation, scope,
classification, retention basis, and lifetime has actually been adopted
(`POST /api/v1/administration/policies`); decision 2's *policy* half was never
removed. `snapshot_provider` (`ackplane-server`) is the one place that shells
out to `pg_dump` and encrypts the result with a locally-generated key
(`ACKPLANE_SNAPSHOT_KEY_PATH`, generated the same way
`ackplane_bridge::load_or_generate_salt` generates its salt) before writing it
under `ACKPLANE_SNAPSHOT_DIR`; the Snapshot capability reports `unavailable`
rather than guessing a location when that variable is unset. It is
deliberately platform-scoped only today: Ackplane's schema is multi-tenant at
the row level, so a `pg_dump` of the whole database is never a valid
tenant-scoped artifact, and a true tenant-scoped export is separate, tracked
follow-on work, not this provider relabeled. Lifecycle purge is the second
implemented privileged operation: a two-phase preview/confirm workflow
(`administration/purge.rs`) against one closed data category today
(`telemetry_events`), tenant- and repository-scoped, requiring its own
adopted `LifecyclePurge` policy. A preview computes and durably records an
impact count and a bounded confirmation window without deleting anything;
only a separate confirm call against that exact request id executes a single
scoped `DELETE ... WHERE tenant_id = $1 AND repository_id = $2 AND
occurred_at < $3` -- never `TRUNCATE`, `DROP DATABASE`, or a schema-wide
statement (ADR-0119 decision 7). Confirming after the window expires, or
after the authorizing policy was revoked, still returns a receipt
(`expired`/`refused`) rather than deleting anything; the receipt itself never
retains the purged rows, only the redacted request/outcome metadata.
ADR-0134 replaces the caller-controlled confirmation label with
domain-separated signatures from enrolled signing keys: preview and confirm
must use distinct public-key fingerprints bound to the same tenant/repository,
each nonce is consumed once, and the receipt retains the verified confirming
key/node/fingerprint provenance. The browser cannot fabricate that proof; an
unsigned, stale, replayed, invalid, wrong-repository, same-key, or
same-key-material confirmation fails before a receipt or deletion. This proves
distinct enrolled key materials, not two different humans; deployments that
need verified human separation still need an identity provider.
Recovery inspection (`administration/recovery.rs`) is the third: ADR-0119
decision 2 never lists it among the privileged classes, so it needs no
adopted policy, only an existing `succeeded` Snapshot receipt to inspect. It
never touches `ACKPLANE_DATABASE_URL`: `snapshot_provider::inspect_snapshot_artifact`
reads the artifact file, recomputes and compares its digest, decrypts
in-process with the installation's own key, and runs `pg_restore --list`
against the decrypted archive purely to confirm it is well-formed -- a
tampered, undecryptable, or corrupt artifact is a reported finding
(`integrity_verified`/`decryption_verified`/`archive_valid`, each independently
false), never an error. Export (`administration/export.rs`) is the fourth and
last privileged class this ADR names: a single request-then-receipt flow
(unlike Purge's two-phase preview/confirm) producing a bounded, redacted
representation of the same `telemetry_events` category for a named purpose,
requiring its own adopted, tenant-scoped `Export` policy.
`export_provider::create_telemetry_export` queries at most the caller's
requested record count, strips every internal identifier
(`telemetry_id`/`node_id`/`agent_session_id`) the same way a `TelemetryEvents`
purge scopes its `DELETE`, and writes a schema-versioned JSON document --
unencrypted, unlike a Snapshot artifact, because the whole point is that it
is already bounded and redacted rather than a second copy of production
state needing the same custody controls as a full-database backup.
Administration policy adoption (`administration/policy.rs`) and the platform
Snapshot request/receipt flow (`administration/snapshot.rs`) are their own
files for the same module-length reason Purge/Recovery/Export already are.
ADR-0111's claim recovery is unchanged.

| Route | Serves |
|---|---|
| `GET /` | The Fleet page (static HTML/JS). |
| `GET /api/v1/fleet` | One page of enrolled repositories for the tenant, with freshness (ADR-0112): optional `q` (substring on repository id, `%`/`_` escaped), `freshness`, `coordination`, `sort` (`field:asc\|desc`, allow-listed), `page`, and `page_size` (clamped 1-100), returning the true filtered `total` alongside the page. Freshness — here, on the repository detail, and on Readiness — compares the projection checkpoint against the head of the stream the projection actually consumes (the last `structural_fact` record), not against the whole-ledger head. The two differ whenever a later record is evidence, knowledge, a claim, a directive, or a delegation, and only the former is a gap a rebuild can close; comparing against the whole-ledger head reported a permanently `Lagging` repository the projection worker could never catch up. |
| `GET /api/v1/agents` | One page of live delegated claims across EVERY repository the tenant has enrolled (`FleetStore::fleet_work`, ADR-0105 decision 5's Agents/Work control room) — the cross-repository "who is working on what, right now" view, distinct from the per-repository `/claims` route below. Each claim carries `has_native_work_task` (`WorkStore::fleet_claims_only_keys` — the cross-repository analogue of `/work`'s per-repository claims-only check), and the response also carries a bounded, tenant-wide `unresolved_waits` list (`WorkStore::fleet_unanswered_waits`) — the same `UnansweredWait` finding Board Doctor reports per repository, read across all of them at once. Optional `repository_id`/`owner_id` (substring, `%`/`_` escaped), `sort` (`field:asc\|desc`, allow-listed: `lease_expires_at`, `repository_id`, `owner_id`), `page`, and `page_size` (clamped 1-100), returning the true filtered `total`. |
| `GET /api/v1/readiness` | One page of per-repository health (`ReadinessStore::readiness`, ADR-0105 decision 6's Workspace/Readiness row) — active node count, freshness, active claim count and soonest lease expiry, and signing-key health, composed entirely from Fleet/Claims/Signing-key state already exposed elsewhere rather than a new domain. Derives a `ready`/`attention_needed`/`not_ready` status per repository: `not_ready` when never projected; `attention_needed` when lagging or any signing key is expired/revoked/unknown/binding-mismatched; `ready` otherwise. `page` and `page_size` (clamped 1-100) only — no filter or sort in this first slice. |
| `GET /api/v1/repositories/:repository_id` | One repository's ledger/projection detail. |
| `GET /api/v1/repositories/:repository_id/timeline` | Its most recent accepted ledger events. |
| `GET /api/v1/repositories/:repository_id/claims` | One fixed-size, 50-item keyset page of its live delegated claims (`FleetStore::active_work`), ordered by `(lease_expires_at, task_id)`. The optional `after_lease_expires_at_micros` and `after_task_id` cursor fields must be supplied together; a nonterminal response returns their next-page value in `next_after`, while a terminal page returns `next_after: null`. |
| `GET /api/v1/repositories/:repository_id/stranded-claims` | One fixed-size, 50-item keyset page of its lease-expired delegated claims (`FleetStore::stranded_claims`) -- the complement of `/claims`, and what `recover` below needs an operator to discover rather than already know. It uses the same paired compound cursor and `next_after` response contract as `/claims`. |
| `GET /work` | The bounded Industrial Work page. It labels each repository's projection as `current`, `claims_only`, or `not_published`, so an empty native Work list never masquerades as a clean task board. Claims-only rows remain lease records with their Ackplane owner, branch, expiry, and declared scope; Bridge never invents Local task title, acceptance, or lifecycle data. A declared path is already an `artifact:` node id, so each row's declared scope links into `GET /graph` seeded on exactly those nodes — declared symbols come along only when they already carry their path, since a bare name has no unambiguous node id. |
| `GET /api/v1/repositories/:repository_id/work` | One bounded page of native Industrial Work records plus a bounded claims-only publication summary. It reads Work and ClaimStore state only after tenant/repository visibility succeeds; it neither imports Local Lodestar tasks nor exposes a mutation. |
| `GET /api/v1/repositories/:repository_id/work/doctor` | Bounded, read-only Work/claim consistency findings including claims without a native Work projection. |
| `POST /api/v1/repositories/:repository_id/work/commands` | ADR-0125: submits one of the ten closed Work commands (`CreateWork`/`RouteWork`/`ReleaseLease`/`AnswerWait`/`SubmitReview`/`Assign`/`Steer`/`Pause`/`Resume`/`Drain`, tagged by `kind` in one JSON envelope) through `WorkCommandService` -- never `WorkStore`/`ClaimStore` directly. Under the Bridge's loopback developer profile every command resolves to a typed `authorization_unavailable` outcome (decision 2): the route and its full request/response contract are real, the authority to execute is not. |
| `POST /api/v1/repositories/:repository_id/work/commands/:command_id/confirm` | Confirms a previously submitted command against its exact payload digest. Same `authorization_unavailable` outcome as submit under the current profile. |
| `GET /design` | The Design Board page (static HTML/JS). |
| `GET`/`POST /api/v1/repositories/:repository_id/designs` | Lists designs, or proposes a new one (`DesignStore::register`) -- a digest-checked idempotent insert of an opaque design record with bounded title/summary/source_version and a closed lifecycle state. |
| `GET /api/v1/repositories/:repository_id/designs/:design_id` | One design's detail. |
| `POST /api/v1/repositories/:repository_id/designs/:design_id/decisions` | Appends a lifecycle transition (`DesignStore::record_decision`) to a design's append-only decision history; nothing here authorizes who may request one (ADR-0121 decision 3 defers that to a future typed command). |
| `POST /api/v1/repositories/:repository_id/designs/:design_id/materializations` | Records a materialization revision (`MaterializationStore::record_materialization`) against a design, idempotency-key-scoped exactly like `evidence_store`. |
| `GET /api/v1/repositories/:repository_id/signing-keys` | Every enrolled signing key, judged as of now (`FleetStore::signing_keys`), reusing `signing_keys::judge` — the same rule an accepted envelope's own verification applies — rather than a second judgment invented for the health view. |
| `GET /delegations` | A tenant-scoped, read-only human delegation authority view. It displays active, expired, and revoked delegation projections with their immutable grant/revocation history; the bounded use-decision subresource below provides the corresponding live authorization audit. It exposes no grant, revoke, approval, credential, idempotency, payload, or remote-execution control. |
| `GET /api/v1/repositories/:repository_id/delegations` | One bounded page of delegation projections in durable `(source_event_position ASC, delegation_id ASC)` order. Optional `limit` is clamped to 1-100; `after_source_event_position` and `after_delegation_id` must be supplied together, and a nonterminal response returns the next boundary in `next_after`. Revoked authority remains visible in the projection rather than disappearing from an operator's review. |
| `GET /api/v1/repositories/:repository_id/delegations/:delegation_id/use-receipts` | One bounded page of immutable live-use decisions in receipt order. Optional `limit` is clamped to 1-100 and `after_receipt_id` is an opaque, tenant/repository-scoped keyset boundary. It exposes the checked delegation version, routine action, bounded token reservation, result, and typed refusal when applicable, but never idempotency keys, request digests, or source payloads. |
| `GET /api/v1/repositories/:repository_id/knowledge` | One fixed-size, 50-item keyset page of active recorded knowledge (`KnowledgeStore::active_page`), ordered by `(confirmed_at DESC, knowledge_id ASC)`. The optional `before_confirmed_at_micros` and `before_knowledge_id` cursor fields must be supplied together; a nonterminal response returns their next-page value in `next_before`, while a terminal page returns `next_before: null`. This deterministic Bridge listing remains distinct from gRPC semantic `recall` ranking. |
| `GET /graph` | The Context Graph page (static HTML/JS): the Memory Plane's projected neighbourhood, rendered as a dependency-free SVG force layout. Edge stroke width and opacity are both derived from `effective_weight`, so decay is something an operator sees rather than a number they have to look up, and the projection's ledger position and rebuild time are shown alongside the graph so a stale projection is legible. Selecting a node lists its edges by weight and can re-seed the traversal from it. The legend doubles as a node-type filter, and the drawn counts are reported against the returned ones so a filtered view never reads as a smaller projection. Accepts `repository`, `seeds`, `depth`, `max_nodes`, and `max_fanout` as query parameters, so another page can link to a focused view. Read-only: the graph is derived from the ledger, so a mutation here could only contradict its own source. |
| `GET /api/v1/repositories/:repository_id/graph` | A bounded Context Graph neighbourhood (`Projector::bounded_neighborhood`, ADR-0087), the same relevance-first traversal the projection worker already implements — wired to the Bridge for the first time rather than a new query. Optional `seeds` (comma-separated node ids; an absent value falls back to `Projector::sample_nodes`, the most recently touched nodes), `depth` (clamped 1-4), `max_nodes` (clamped 1-300), and `max_fanout` (clamped 1-30). Each edge's `effective_weight` is computed at response time from `base_weight`/`half_life_hours`/`updated_at`, mirroring `mindleak_core::decay::effective_weight` exactly — never stored. |
| `GET /api/v1/repositories/:repository_id/constitution` | Its published constitution snapshot, if any (`ConstitutionStore::get_active`) with immutable publication source/digest metadata when that version was recorded — read-only; no adopt/tailor/reject/promote/waiver action is exposed here. |
| `GET`/`POST /api/v1/repositories/:repository_id/constitution/proposals`, `POST .../proposals/:proposal_id/withdraw` | ADR-0126: list, submit, and withdraw a Bridge-originated proposed clause change (`ConstitutionStore::list_proposals`/`propose_clause`/`withdraw_proposal`). The only write path this domain exposes, and it only ever touches `constitution_proposals` — never `constitution_publications`, the table only a repository's own local `amend_constitution` writes. Withdrawal is gated to the proposal's own author. |
| `GET /constitution` | A static, visible-refresh-only tenant Constitution page. It renders the selected repository's published version, source/digest provenance, and bounded clauses through the existing constitution endpoint, plus (ADR-0126) a propose-a-clause form and a rendered pending/withdrawn proposal list; it does not inspect local Lodestar storage, and every mutation it can make targets the proposals sub-resource above, never the published constitution endpoint itself. |
| `GET /api/v1/repositories/:repository_id/telemetry` | Its per-name current health (`TelemetryStore::read`, ADR-0105 decision 6) — lifetime `calls`/`errors` alongside `currently_failing`, derived from the most recent success/error rather than the lifetime count, so a resolved past error stops reading as an active fault once a later call succeeds. Rendered as tenant-scoped health cards, grouped by kind, at `GET /telemetry` (static HTML/JS), including bounded bucketed sparkline history and a newest-first diagnostic sample of at most 50 recent accepted telemetry events from the same authoritative store. |
| `POST /api/v1/repositories/:repository_id/tasks/:task_id/recover` | Bridge's first claim **mutation** (ADR-0111): recovers a stranded claim by calling `ClaimStore::recover` directly, tenant-scoped and reason-required. `delegate`, `release`, and `renew` remain node-signed-only and are not exposed here. The handler resolves `expected_owner` itself via the new `FleetStore::claim_owner` (unlike `active_work`, this does not filter out an already-expired lease), rather than trusting a caller-supplied value. |
| `GET /administration` | The Administration page (static HTML/JS): tenant-scoped status, the operation-capability list, and — only when `claim_recovery` reports `available` — the stranded-claim recovery workflow above, reusing the same `/stranded-claims` and `/recover` routes rather than a second implementation. |
| `GET /api/v1/repositories/:repository_id/administration/status` | ADR-0119's status/inspection class: projection freshness and ledger position (`FleetStore::repository`), an honestly `not_reported` durability report, and the fixed six-operation capability list (`status_inspection`, `snapshot`, `export`, `claim_recovery`, `recovery_inspection`, `lifecycle_purge`) naming which are available, refused, or unavailable and why under the current loopback profile. |
| `POST /api/v1/administration/policies` | ADR-0119 decision 2 / ADR-0128: adopts a new `AdministrationPolicy` naming its operation, platform-or-tenant scope, data classification, retention basis, and bounded lifetime, attributed to the loopback developer profile's own salted token (`administration_store::AdministrationStore::adopt_policy`). An identical resubmission under the same idempotency key replays the original record; a changed one conflicts (`409`). |
| `POST /api/v1/administration/snapshots` | Requests, then synchronously executes, a platform Snapshot under an already-adopted, still-active policy (`administration_store::request_snapshot` + `snapshot_provider::create_platform_snapshot`): refuses (`409`) before any request row exists if no active policy authorizes it, otherwise runs `pg_dump --format=custom`, encrypts the artifact, and records an immutable receipt (outcome, reason, artifact path, manifest digest, encryption key id, size, and verified flag) that a retried request replays rather than re-executes. |
| `GET /api/v1/administration/snapshots/:request_id` | The receipt recorded for one prior Snapshot request, if any. |
| `POST /api/v1/repositories/:repository_id/administration/purges` | ADR-0119 decisions 7/9: previews a Lifecycle purge -- refuses (`409`, no request row created) without an active tenant-scoped `LifecyclePurge` policy, otherwise counts matching `telemetry_events` rows and records a bounded, expiring, idempotent preview naming the exact cutoff and confirmation window. |
| `POST /api/v1/repositories/:repository_id/administration/purges/:request_id/confirm` | Executes (or refuses/expires) a previously previewed purge. A confirmation past its window, or against a since-revoked policy, still returns a receipt (`expired`/`refused`) rather than deleting anything; only a fresh preview restarts the clock. Idempotent: re-confirming an already-receipted request replays it rather than deleting a second time. Tenant- and repository-scoped: only the principal and repository that made the request may confirm or read it. |
| `GET /api/v1/repositories/:repository_id/administration/purges/:request_id` | The receipt recorded for one prior purge request, if any, under the same ownership rule as confirm. |
| `POST /api/v1/administration/snapshots/:request_id/inspect` | ADR-0119 decision 6: inspects the Snapshot artifact recorded for `request_id` -- digest integrity, decryptability with the installation's own key, and `pg_restore --list` archive validity -- and durably records a new report. Refuses (`409`) when the request has no `succeeded` receipt to inspect. Needs no adopted policy: inspection is read-only and never touches production authority. |
| `GET /api/v1/administration/snapshots/:request_id/inspect` | The most recently recorded inspection report for a Snapshot request, if any, tenant-scoped like every other Snapshot/purge read. |
| `POST /api/v1/repositories/:repository_id/administration/exports` | ADR-0119 decision 5: requests, then synchronously executes, a bounded/redacted Export of `telemetry_events` -- refuses (`409`, no request row created) without an active tenant-scoped `Export` policy, otherwise queries at most `max_records` rows, redacts every internal identifier, and records a `succeeded`/`failed` receipt naming its schema version, record count, and exactly which fields were redacted. Idempotent like Snapshot: a replayed request returns the original receipt rather than re-running the query. |
| `GET /api/v1/repositories/:repository_id/administration/exports/:request_id` | The receipt recorded for one prior Export request, if any, tenant- and repository-scoped like every other Snapshot/purge/export read. |
| `GET /static/shared/chrome.css` / `GET /static/shared/chrome.js` | The one shared brand-mark and grouped-nav asset every static page loads (ADR-0124), served by `shared_assets.rs` with the correct `Content-Type`. `chrome.js`'s `NAV_ITEMS` is the single declared list of every ADR-0105 decision 5 capability; `chrome.css` styles it through six neutral `--chrome-*` custom properties that each page bridges onto its own palette. A page's entire brand/nav footprint is two mount points (`[data-bridge-brand]`, `[data-bridge-nav]`) plus these two tags — never its own copy of the nav's markup, CSS, or disclosure script. |

Neither Ackplane nor Bridge speak MCP today — the former is gRPC-only, the
latter HTTP-only — so no MCP client (an AI coding agent, not a browser) can
reach the Industrial profile directly. ADR-0136 (Accepted) closes that gap
with `ackplane-mcp`: an MCP stdio server over the same gRPC services, never a
second, Postgres-side reimplementation of `mindleak-core`/`lodestar-core`'s
business rules -- every handler translates to an Ackplane RPC that already
exists and is already authorized, deciding nothing about enrolment or
signatures itself. Its tool surface today is `open_session`,
`check_enrollment_status`, and `active_claims`. `open_session` implements
ADR-0137 clause 2: identity is the session, not the process, sharing the same
`mindleak-session` crate `mindleak-mcp`/`lodestar-mcp` already use, so it
mints the same `session:v1:<hex>` form and accepts the same declared
working-context fields; it consults no Ackplane endpoint, since opening a
session grants no arbiter-side authority by itself. `check_enrollment_status`
translates `NodeEnrollmentService.CheckEnrollmentStatus` (ADR-0122).
`active_claims` translates `ClaimDelegationService.ListActiveClaims`
(ADR-0096, ADR-0139 clause 1's read half) -- read-only and unsigned, since it
asks only what the arbiter already states about its own arbitration. It
holds itself to a local-loopback Ackplane endpoint until ADR-0137 clause 1
lands: today neither translated tool authenticates its *connection* over
NodeSync the way `ackplane-supervisor` does (only per-operation signing, or
no signing at all by design), so `endpoint::resolve_endpoint` refuses a
non-loopback target rather than send a possession proof somewhere that
decision has not been made about yet. A `task_query`-named tool reading
Industrial Work's broader projection (list/detail/history/waits/checkpoints/
overlap/stalled, ADR-0139 clause 2) and a `pgvector`-backed recall store
scoped to `projected_nodes` rather than the curated `knowledge` domain
(ADR-0140) both remain open before this front door is usable end-to-end
beyond claim arbitration and enrolment status.

### `editors/vscode` (extension)

Passive editor, shell-execution, workspace-mutation, and Git commit sensors plus
a Cytoscape visualizer. It spawns `mindleak-mcp` as a child process and speaks
the same MCP protocol. Stable shell execution events require VS Code 1.93;
unsupported shells are visibly degraded rather than inferred from terminal text.
Platform-targeted VSIX packages contain both native servers under `bin/` and
report memory, intent, terminal, and Git health independently (ADR-0016). A
Telemetry pane renders a derived, real-time effectiveness readout (graph size,
tool success/error rates, latency, per-tool metrics) from `graph_stats` and
`telemetry_snapshot`, with opt-in live event logging; the derivations are the
pure helpers in `src/util.ts`.
The Workspace readiness tree follows the same derived-state rule: pure
`src/readiness.ts` maps MCP initialize identities, `graph_stats`, `board`,
`design_query`, and sensor health to one next action; `readinessController.ts`
performs those reads and `readinessViewProvider.ts` is thin VS Code rendering.
Only the one-time teaching-view dismissal uses workspace state; no graph or
intent authority is copied into the extension.
The Work view's allocation flow collects optional concrete paths/symbol ids,
combines both ADR-0024 overlap reads, and shows scoped work as a planning hint;
warnings remain explicitly overridable. Review-needed rows call `task_transition`
(`to="resolve"` / `to="reopen"`) and the `conformance_history` tool in place; the
complete Evidence Board remains an advanced, hidden-by-default audit view
(ADR-0040).

### `clients/node/mindleak-client` (package)

A packaged, installable Node.js client for either MCP stdio server (ADR-0103),
replacing the pattern of every consumer hand-writing its own JSON-RPC framing —
the problem this closed first-hand while bootstrapping CompLeak. `McpConnection`
(`protocol.ts`) is the transport: newline-delimited JSON-RPC request/response
correlation by id, `notify` for one-way messages, and per-request timeouts,
independent of any specific server binary. `MindLeakClient` (`client.ts`) wraps
it with the `initialize` handshake, `open_session` (ADR-0030), and typed
`callTool`; it spawns the command it is given rather than bundling or assuming a
server binary. Thin per-domain services (`services/`) — `KnowledgeService`,
`TaskService`, `EvidenceService`, `GraphService` — wrap individual tool calls
without adding tool surface of their own. `parseToolResult` (`util.ts`) is the
shared `structuredContent`-preferring result parser (ADR-0027), extracted as a
pure function so it is unit-testable without a real subprocess. MindLeak ships
the generic client and transport; a domain-specific consumer supplies its own
services against the same protocol, mirroring the ADR-0101 boundary rule of
"generic belongs here, domain-specific belongs downstream." CI's
`reference-consumer` job (ADR-0104) spawns both servers through this same
client and exercises one representative read-only call per tool family, so a
breaking change to either tool surface is caught before merge rather than
surfacing first as this package's own failure.

## Data model

- **Nodes** — `symbol` · `artifact` · `execution` · `intent` · `agent` ·
  `package` (ADR-0006). Ids are stable and human-readable
  (`artifact:src/auth.ts`, `symbol:src/auth.ts:validateSession`).
- **Edges** — directional, decay-weighted: `contains` · `calls` · `modified` ·
  `failed_on` · `refactored` · `relates_to` · `observed` · `imports` ·
  `extends` · `implements` · `depends_on` (ADR-0006 phases 1-3).

## Decay

Effective weight is computed at query time, never by rewriting rows:

```
W_effective = W_base · 2^(−Δt_hours / (half_life_hours · signal_multiplier))
```

Raw execution evidence uses a 24h half-life; human intent 168h; default 48h.
Edges below the resolved threshold (`0.05` by default) are ignored in queries
and purged by `prune_graph`. Base half-lives and the threshold can be tuned in a
strict `.mindleak.toml` or by environment (ADR-0014); the immutable policy is
loaded once and applied at read time. Re-ingesting an edge reinforces it
(`+0.05`, capped at 1.0) and resets its decay clock. Structural edges additionally carry artifact ownership:
re-ingesting a file replaces that owner's structural snapshot, retracting facts
that disappeared (ADR-0007). `boost_entity` changes attention without refreshing
unrelated incident evidence.

**Signal-weighted decay (ADR-0005/0012).** At query/prune time, `GraphStore`
derives raw `SignalEvidence` from reinforcement span, independent source classes,
failure/change/success consequence, surprise, incoming structural degree, and
explicit decisions. `decay::signal_multiplier` maps those proxies to a bounded
1x-8x half-life multiplier. Returned edges expose the evidence/multiplier for
auditability; neither multiplier nor effective weight is stored. Near-expiry
high-signal episodics are returned by `prune_graph`; expired candidates remain
inactive but retained until optional `consolidate_signal` persists an intent and
acknowledges them, leaving model access off deterministic maintenance.

**Working memory (ADR-0017 phase 1).** `GraphStore::working_set` derives a
per-agent, capacity-bounded focus view from active `observed` edges. No buffer or
LRU is persisted. Repeated observations spanning the existing signal window
become rehearsal evidence only while the target remains inside that agent's
top-K; the write path remains zero-token.

**Compiled context (ADR-0102).** `MindLeak::compile_context` composes `recall`,
`working_set`, and optionally `evidence_for` into one bounded, ranked,
token-budgeted packet — no new source of truth, only ranking (each source's own
already-decayed relevance score) and a `max_tokens` (bytes/4) budget applied
highest-ranked-first. `budget_report.excluded` names what was cut for budget
reasons alone. `governing` is a caller-supplied pass-through of Lodestar's
`advise()` result: this crate has no dependency on `lodestar-core`, so
cross-plane data arrives as an argument, the same seam `promote_signals`
already crosses.

**Compiled digests (ADR-0101).** `MindLeak::compile_digest` renders a named,
typed digest — a playbook/runbook/repository-guide — from current graph state
through a deterministic template, storing the result as a `digest` node
(`NodeType::Digest`) rather than hand-authored prose. A digest is never edited:
to change one, change what it compiles from and recompile, the same discipline
ADR-0056 already applies to the changelog, generalised to any typed document.
`MindLeak::digest_status` reports whether a digest's recorded source snapshot
still matches live graph state — `current` while every source node id it read
still exists, `stale` once any has been forgotten or reaped — and never
regenerates the digest on its own.

**Autonomous consolidation (ADR-0017 phase 2).** An off-by-default scheduler in
`mindleak-mcp` tracks stdio request activity with a condition variable. After a
bounded idle period it calls the same `MindLeak::consolidate_signal` path through
a second file-backed SQLite connection. Model output becomes deterministic graph
facts; one optimistic transaction stores the gist/provenance and deletes only
candidate edge versions that have not changed meanwhile. Every attempt emits
maintenance telemetry. A persisted workspace lease gates both manual and idle
model calls immediately before inference, preventing duplicate spend across MCP
processes. EOF wakes waiting workers; a bounded grace joins normal exits while a
currently blocked HTTP attempt may be abandoned for process termination without
post-cancellation persistence.

## Ingestion (zero-token)

All write-path extraction is pure pattern matching:

- **execution** — command + exit code → `execution` node; changed files →
  `modified` edges; stack-trace `path:line` regex on failure → `failed_on` edges.
- **git** — commit → `intent` node; changed files → `refactored` edges;
  `DECISION:`/`HACK:`/`WHY:` markers extracted into node content.
- **ast** — heuristic extraction (pattern-based per language) → `symbol` nodes +
  `contains` edges, plus **in-file `calls` edges** (a definition body referencing
  another symbol defined in the same file). The complete result transactionally
  replaces the artifact's prior structural snapshot. Structured behind a
  swappable interface; Tree-sitter is the precision upgrade for cross-file/scoped
  calls.
- **structure** (ADR-0006) — shipped phases 1-2 parse static
  JavaScript/TypeScript `import` and `require` declarations into `imports`,
  `package`, and named cross-file `calls` facts, plus simple named class/interface
  heritage into `extends`/`implements`. A lightweight lexer excludes comments,
  strings, templates, member calls, generic constraints, and basic lexical
  shadowing. Unresolved relative targets store deterministic candidate ids;
  ingesting a real candidate atomically retargets structural symbol edges and
  removes the stub.
- **manifest** (ADR-0006 phase 3) — direct dependencies from `Cargo.toml`,
  `package.json`, `go.mod`, and `requirements*.txt` become artifact-to-package
  `depends_on` edges. TOML, JSON, and PEP 508 use structured parsers; Go uses its
  narrow `require` grammar. Re-ingestion retracts removed dependencies, while a
  malformed supported manifest fails before replacing its last valid snapshot.

## Optional model layer

`consolidate.rs` calls the configured OpenAI-compatible
`/v1/chat/completions` endpoint with a JSON `response_format` to compress a batch
of raw logs into a single `intent` node. It is asynchronous and never on the hot
path. The default endpoint is local; selecting a remote endpoint deliberately
sends the bounded request to that service.

See [SPEC.md](SPEC.md) for the full design rationale.
