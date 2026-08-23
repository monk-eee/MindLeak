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
| [`ingest/`](../crates/mindleak-core/src/ingest/mod.rs) | Zero-token deterministic extractors: `execution`, `git`, `ast`, `structure/{imports,hierarchy}` (JS/TS imports and type hierarchy), `javascript/` (the JS/TS lexer plus `nav`, `scope`, `callable`, `binding`, `shadowing`), and `manifest` (direct package dependencies). |
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

### `ackplane-supervisor` (library)

The local durable directive inbox and outbox for one enrolled supervisor and worker session (ADR-0116). It owns a caller-provided SQLite database rather than either repository-local plane database. The inbox binds itself to one tenant, repository, node, supervisor, and agent session; verifies that an incoming directive targets that identity and names an advertised capability; rejects expired or out-of-sequence delivery; and persists accepted, capability-refused, and expired receipts before returning them. Replaying the same directive id and payload digest returns the original stored receipt, while a changed digest under the same id is refused without overwriting evidence.

The outbox persists encoded `NodeFrame`s before a future sender may transmit them. It assigns a local positive contiguous sequence through a persisted high-water mark, replays an identical frame idempotently, refuses a changed frame at an existing sequence, returns pending frames in sequence order under a bounded limit, and prunes only frames at or below a caller-supplied acknowledged sequence. The inbox and outbox share the same durable supervisor identity/session binding so a different node or worker session cannot reopen the queue database.

It does not open a network listener, spawn a worker, execute a directive, or offer a shell/process abstraction. A future NodeSync transport adapter reads pending outbound frames, acknowledges accepted delivery, and feeds received directives into the inbox; a future worker adapter reports application or checkpoint effects. This crate establishes the local durable queue boundary those adapters must preserve.


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
directive envelopes before any future sender can deliver them. `DirectiveReceipt` records append separately only when their
tenant, repository, node, session, sequence, and payload digest bind exactly
to the stored directive; identical retries replay their original record. This
foundation has no Bridge command route, does not issue a directive from an
unauthenticated principal, and does not yet send an outbound stream frame.

Authenticated `NodeSync` streams now ingest `DirectiveReceipt` frames through that same ledger only after their tenant, repository, and node match the completed connection challenge. Ackplane returns a typed `SupervisorFrameReceipt` for a durable write or exact replay, resolving the supervisor only from the receipt's scoped session; unknown or cross-scope directives receive a generic refusal rather than a directive-existence disclosure. This remains receipt ingress only: it adds neither outbound directive delivery nor a Bridge issuance route.

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

`ConstitutionService` (`constitution_store.rs`/`constitution_service.rs`) is a
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

`DesignStore` (`design_store.rs`) is a separate Industrial-only authority
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
future typed command. Nothing here yet exposes an RPC/HTTP route,
materialization plans/revisions (decision 4), or a Bridge Design Board
(decision 7) — those are separate follow-on slices.

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
is predominantly read-only. ADR-0111's tenant-scoped, reason-required
stranded-claim recovery is the sole current Bridge mutation; enrolment, normal
claim lifecycle, and ledger records remain behind Ackplane's typed service
boundaries. Current routes, each 404 on a repository the tenant has not
enrolled rather than leaking a distinguishable error:

| Route | Serves |
|---|---|
| `GET /` | The Fleet page (static HTML/JS). |
| `GET /api/v1/fleet` | One page of enrolled repositories for the tenant, with freshness (ADR-0112): optional `q` (substring on repository id, `%`/`_` escaped), `freshness`, `coordination`, `sort` (`field:asc\|desc`, allow-listed), `page`, and `page_size` (clamped 1-100), returning the true filtered `total` alongside the page. |
| `GET /api/v1/agents` | One page of live delegated claims across EVERY repository the tenant has enrolled (`FleetStore::fleet_work`, ADR-0105 decision 5's Agents/Work control room) — the cross-repository "who is working on what, right now" view, distinct from the per-repository `/claims` route below. Optional `repository_id`/`owner_id` (substring, `%`/`_` escaped), `sort` (`field:asc\|desc`, allow-listed: `lease_expires_at`, `repository_id`, `owner_id`), `page`, and `page_size` (clamped 1-100), returning the true filtered `total`. |
| `GET /api/v1/readiness` | One page of per-repository health (`ReadinessStore::readiness`, ADR-0105 decision 6's Workspace/Readiness row) — active node count, freshness, active claim count and soonest lease expiry, and signing-key health, composed entirely from Fleet/Claims/Signing-key state already exposed elsewhere rather than a new domain. Derives a `ready`/`attention_needed`/`not_ready` status per repository: `not_ready` when never projected; `attention_needed` when lagging or any signing key is expired/revoked/unknown/binding-mismatched; `ready` otherwise. `page` and `page_size` (clamped 1-100) only — no filter or sort in this first slice. |
| `GET /api/v1/repositories/:repository_id` | One repository's ledger/projection detail. |
| `GET /api/v1/repositories/:repository_id/timeline` | Its most recent accepted ledger events. |
| `GET /api/v1/repositories/:repository_id/claims` | Its live delegated claims (`FleetStore::active_work`). |
| `GET /api/v1/repositories/:repository_id/stranded-claims` | Its lease-expired delegated claims (`FleetStore::stranded_claims`) -- the complement of `/claims`, and what `recover` below needs an operator to discover rather than already know. |
| `GET /api/v1/repositories/:repository_id/signing-keys` | Every enrolled signing key, judged as of now (`FleetStore::signing_keys`), reusing `signing_keys::judge` — the same rule an accepted envelope's own verification applies — rather than a second judgment invented for the health view. |
| `GET /api/v1/repositories/:repository_id/knowledge` | Its recorded knowledge, recency-ordered (`KnowledgeStore::recall`, ADR-0106) — the same query the knowledge domain already exposes over gRPC, not a second one invented for the Bridge view. |
| `GET /api/v1/repositories/:repository_id/graph` | A bounded Context Graph neighbourhood (`Projector::bounded_neighborhood`, ADR-0087), the same relevance-first traversal the projection worker already implements — wired to the Bridge for the first time rather than a new query. Optional `seeds` (comma-separated node ids; an absent value falls back to `Projector::sample_nodes`, the most recently touched nodes), `depth` (clamped 1-4), `max_nodes` (clamped 1-300), and `max_fanout` (clamped 1-30). Each edge's `effective_weight` is computed at response time from `base_weight`/`half_life_hours`/`updated_at`, mirroring `mindleak_core::decay::effective_weight` exactly — never stored. |
| `GET /api/v1/repositories/:repository_id/constitution` | Its published constitution snapshot, if any (`ConstitutionStore::get_active`) with immutable publication source/digest metadata when that version was recorded — read-only; no adopt/tailor/reject/promote/waiver action is exposed here. |
| `GET /constitution` | A static, visible-refresh-only tenant Constitution page. It renders the selected repository's published version, source/digest provenance, and bounded clauses through the existing constitution endpoint; it does not inspect local Lodestar storage or expose a mutation. |
| `GET /api/v1/repositories/:repository_id/telemetry` | Its per-name current health (`TelemetryStore::read`, ADR-0105 decision 6) — lifetime `calls`/`errors` alongside `currently_failing`, derived from the most recent success/error rather than the lifetime count, so a resolved past error stops reading as an active fault once a later call succeeds. Rendered as tenant-scoped health cards, grouped by kind, at `GET /telemetry` (static HTML/JS), including bounded bucketed sparkline history from the same authoritative query. |
| `POST /api/v1/repositories/:repository_id/tasks/:task_id/recover` | Bridge's first claim **mutation** (ADR-0111): recovers a stranded claim by calling `ClaimStore::recover` directly, tenant-scoped and reason-required. `delegate`, `release`, and `renew` remain node-signed-only and are not exposed here. The handler resolves `expected_owner` itself via the new `FleetStore::claim_owner` (unlike `active_work`, this does not filter out an already-expired lease), rather than trusting a caller-supplied value. |

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
