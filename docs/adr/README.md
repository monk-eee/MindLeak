# Architecture Decision Records

This log captures decisions that are **hard to reverse or surprising** — the
kind someone might otherwise "simplify" back into a bug. Each ADR is dated and
immutable; supersede rather than edit.

Format: [MADR](https://adr.github.io/madr/)-lite. Keep them short.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](0002-sqlite-decay-over-vector-llm.md) | SQLite + half-life decay over vector-only / per-event LLM memory | Accepted |
| [0003](0003-agent-attribution-as-observed-edges.md) | Agent attribution as decay-weighted `observed` edges | Accepted |
| [0004](0004-intent-plane-spec-brain.md) | Intent Plane: a durable "spec brain" separate from the decay graph | Accepted |
| [0005](0005-signal-weighted-decay.md) | Signal-weighted decay ("decay noise, not signal") | Accepted |
| [0006](0006-structural-dependency-edges.md) | Structural & dependency edges (graph enrichment for impact analysis) | Accepted |
| [0007](0007-structural-snapshot-reconciliation.md) | Structural snapshots replace owned facts | Accepted |
| [0008](0008-semantic-recall-embedding-index.md) | Optional semantic recall via a local embedding index (complements, never replaces, the decay graph) | Accepted |
| [0009](0009-evidence-backed-conformance.md) | Evidence-backed conformance across the memory and intent planes | Accepted |
| [0010](0010-observability-and-resilience.md) | Observability, telemetry, and network resilience | Accepted |
| [0011](0011-passive-terminal-and-git-sensors.md) | Passive terminal and Git evidence sensors | Accepted |
| [0012](0012-derived-signal-evidence.md) | Derived bounded signal evidence | Accepted |
| [0013](0013-local-data-lifecycle.md) | Local data backup, export, and reset lifecycle | Accepted |
| [0014](0014-per-project-decay-configuration.md) | Per-project decay configuration | Accepted |
| [0015](0015-advisory-symbol-leases.md) | Progressive task handoffs before advisory symbol leases | Accepted (no symbol-lease primitive) |
| [0016](0016-platform-packaging-and-registration.md) | Platform packaging and workspace registration | Accepted |
| [0017](0017-working-memory-and-autonomous-consolidation.md) | Working-memory tier and autonomous consolidation cycle | Accepted (implemented) |
| [0018](0018-conflict-safe-concurrent-editing.md) | Conflict-safe concurrent editing in a shared working tree (worktrees optional) | Superseded by [ADR-0032](0032-single-checkout-fleet-integration.md) |
| [0019](0019-task-retention-and-board-hygiene.md) | Task retention and board hygiene - hide, never delete | Accepted |
| [0020](0020-task-lifecycle-states.md) | Task lifecycle state machine — `needs_input` and `paused` | Accepted |
| [0021](0021-node-lifecycle-and-reaping.md) | Node lifecycle and maintenance reaping semantics | Accepted |
| [0022](0022-learned-knowledge-loop.md) | The learned-knowledge loop — promotion, revalidation, and advisory conformance | Accepted |
| [0023](0023-design-board-accept-bridge.md) | Design items, the Design Board, and reviewed materialization | Accepted |
| [0024](0024-preflight-overlap-detection.md) | Pre-flight work-overlap detection across both planes | Accepted |
| [0025](0025-authoritative-checked-conformance.md) | Authoritative checked conformance | Accepted |
| [0026](0026-constitutional-policy-over-mechanistic-ratchets.md) | Constitutional policy over mechanistic ratchets | Accepted |
| [0027](0027-extension-led-progressive-disclosure.md) | Extension-led progressive disclosure over MCP primitives | Accepted |
| [0028](0028-external-adoption-evidence-gate.md) | External adoption evidence before broad product claims | Accepted |
| [0029](0029-proactive-constitutional-advice.md) | Proactive constitutional advice (ask-before-act) | Accepted |
| [0030](0030-discrete-per-agent-identity.md) | Discrete per-agent identity for concurrent coordination | Accepted |
| [0031](0031-exportable-conformance-evidence.md) | Exportable conformance evidence, the Evidence Board, and a CI conformance gate | Accepted |
| [0032](0032-single-checkout-fleet-integration.md) | Single-checkout, single-publisher fleet integration | Superseded by [ADR-0038](0038-isolated-worktrees-shared-repository-state.md) |
| [0033](0033-copilot-cli-registration.md) | First-class GitHub Copilot CLI registration for both planes | Accepted |
| [0034](0034-typed-controls-and-enforcement-ceilings.md) | Typed controls, workflow scope, and enforcement ceilings | Accepted |
| [0035](0035-fleet-management-heuristics.md) | Fleet management heuristics and feedback | Accepted |
| [0036](0036-forbid-change-is-its-own-consequence.md) | A `forbid_change` lock is its own consequence declaration | Accepted |
| [0037](0037-ratchet-baselines-are-not-self-adopted.md) | A ratchet never sets its own baseline | Accepted |
| [0038](0038-isolated-worktrees-shared-repository-state.md) | Isolated worktrees, shared repository state, reviewed convergence | Accepted |
| [0039](0039-waivers-end-amendments-change.md) | Every waiver ends; changing the rule is an amendment | Accepted |
| [0040](0040-one-work-surface.md) | One Work surface with advanced proof | Accepted |
| [0041](0041-cross-cutting-work-is-declared.md) | Cross-cutting work is declared, not waived | Accepted |
| [0042](0042-designs-are-retired-by-a-person.md) | A design is retired by a person, never by a missing file | Accepted |
| [0043](0043-adoption-into-active-constitution-is-an-amendment.md) | Adopting a pack clause into an active constitution is an amendment | Accepted |
| [0044](0044-declared-context-is-durable.md) | Declared context is durable, and staleness is declared too | Accepted |
| [0045](0045-a-fleet-is-a-distributed-system.md) | An agent fleet is a distributed system, not a team | Accepted |
| [0046](0046-agents-talk-through-the-durable-thread.md) | Agents talk through the durable thread, never to each other | Accepted |
| [0047](0047-a-status-is-not-a-decision.md) | A status reflects a decision; only a decision records one | Accepted |
| [0048](0048-a-lapsed-lease-holes-the-window-it-does-not-move-it.md) | A lapsed lease holes the evidence window, it does not move it | Accepted |
| [0049](0049-publication-requires-a-claim.md) | Publication requires a claim; the ledger is not optional | Accepted |
| [0050](0050-a-superseded-decision-is-not-a-stale-one.md) | A superseded decision is not a stale one | Accepted |
| [0051](0051-a-decision-already-made-can-still-be-signed.md) | A decision already made can still be signed | Accepted |
| [0052](0052-a-lease-is-a-heartbeat-not-a-deadline.md) | A lease is a heartbeat, not a deadline | Accepted |
| [0053](0053-the-graph-records-events-not-conclusions.md) | The graph records events, not conclusions | Accepted |
| [0054](0054-identity-is-the-session-not-the-process.md) | Identity is the session, not the process that hosts it | Accepted |
| [0055](0055-draft-the-question-decide-nothing.md) | Draft the question, decide nothing | Accepted |
| [0056](0056-the-changelog-is-assembled-not-edited.md) | The changelog is assembled, not edited | Accepted |
| [0057](0057-work-already-done-is-a-collision.md) | Work already done is a collision the fleet cannot see | Accepted |
| [0058](0058-work-that-shipped-must-leave-the-board.md) | Work that shipped must be able to leave the board | Accepted |
| [0059](0059-the-tool-surface-is-a-vocabulary.md) | The tool surface is a vocabulary, not an inventory | Accepted |
| [0060](0060-work-whose-product-is-not-code-must-still-conform.md) | Work whose product is not code must still be able to conform | Accepted |
| [0061](0061-delivery-is-queued-not-raced.md) | Delivery is queued, not raced | Accepted (remedy blocked) |
| [0062](0062-the-delivery-queue-is-ours-to-run.md) | The delivery queue is ours to run | Accepted |
| [0063](0063-a-migration-may-tidy-the-past-never-the-present.md) | A migration may tidy the past, never the present | Accepted |
| [0064](0064-the-log-is-the-ledger.md) | The log is the ledger | Accepted |
| [0065](0065-completion-belongs-at-the-publication-boundary.md) | Completion belongs at the publication boundary | Accepted |
| [0066](0066-retrieval-rides-on-the-question-already-asked.md) | Retrieval rides on the question already asked | Accepted |
| [0067](0067-a-claim-is-a-statement-that-you-are-working-on-something.md) | A claim is a statement that you are working on something | Accepted |
| [0068](0068-an-amendment-carries-the-work-it-renames.md) | An amendment carries the work it renames | Accepted |
| [0069](0069-resolutions-that-predate-attribution-are-accepted-as-historical.md) | Resolutions that predate attribution are accepted as historical | Accepted |
| [0070](0070-paused-work-must-find-its-owner-or-a-successor.md) | Paused work must find its owner or a successor | Accepted |
| [0071](0071-task-resolution-records-an-unverified-reviewer-label.md) | Task resolution records an unverified reviewer label | Accepted |
| [0072](0072-an-advisory-informs-it-does-not-cap-the-verdict.md) | An advisory informs; it does not cap the verdict | Accepted |
| [0073](0073-each-window-roots-its-servers-at-the-worktree-it-edits.md) | Each window roots its servers at the worktree it edits | Accepted |
| [0074](0074-coverage-is-a-prediction-until-conformance-speaks.md) | Coverage is a prediction until conformance speaks | Accepted |
| [0075](0075-a-hit-must-stand-out-from-its-own-querys-field.md) | A hit must stand out from its own query's field | Accepted |
| [0076](0076-evidence-is-judged-against-the-window-that-authorised-it.md) | Evidence is judged against the window that authorised the work | Accepted |
| [0077](0077-a-crowded-board-is-not-a-decision.md) | A crowded board is not a decision | Accepted |
| [0078](0078-an-unbound-file-is-reported-at-publication.md) | An unbound file is reported at publication | Accepted |
| [0079](0079-a-model-call-must-fail-loudly-or-it-fails-silently.md) | A model call must fail loudly, or it fails silently | Accepted |
| [0080](0080-knowledge-is-searched-where-it-is-already-read.md) | Knowledge is searched where it is already read | Accepted |
| [0081](0081-agent-memory-is-a-staging-area-not-a-silo.md) | Agent memory is a staging area, not a silo | Accepted |
| [0082](0082-ackplane-is-a-standalone-federation-service.md) | Ackplane is a standalone federation service | Accepted |
| [0083](0083-grpc-is-the-ackplane-node-protocol.md) | gRPC is the Ackplane node protocol | Accepted |
| [0084](0084-ackplane-evidence-has-explicit-trust.md) | Ackplane evidence has explicit trust | Accepted |
| [0085](0085-node-enrolment-requires-proof-of-possession.md) | Node enrolment requires proof of possession | Accepted |
| [0086](0086-postgresql-is-the-ackplane-ledger-and-arbiter.md) | PostgreSQL is the Ackplane ledger and arbiter | Accepted |
| [0087](0087-the-ackplane-graph-is-a-projection-not-an-authority.md) | The Ackplane graph is a projection, not an authority | Accepted |
| [0088](0088-the-ackplane-runs-in-containers-the-planes-do-not.md) | Ackplane runs in containers; the planes do not | Accepted |
| [0089](0089-mindleak-is-an-operating-system-for-agent-coordination.md) | MindLeak is an operating system for agent coordination | Accepted |
| [0090](0090-certification-is-a-status-not-a-service.md) | Certification is a status, not a service | Accepted |
| [0091](0091-ackplane-builds-and-tests-without-a-database.md) | Ackplane builds and tests without a database | Accepted |
| [0092](0092-ackplane-is-governed-by-its-own-goal.md) | Ackplane is governed by its own goal | Accepted |
| [0093](0093-tool-descriptions-are-a-contract-not-a-narrative.md) | Tool descriptions are a contract, not a narrative | Accepted |
| [0094](0094-the-bridge-preserves-standalone-operation.md) | The Bridge preserves standalone operation | Superseded by [ADR-0105](0105-bridge-is-the-server-version-of-the-vsix.md) |
| [0095](0095-the-bridge-uses-an-authenticated-projection-api.md) | The Bridge uses an authenticated projection API | Accepted |
| [0096](0096-ackplane-arbitrates-federated-claims-through-leased-delegation.md) | Ackplane arbitrates federated claims through leased delegation | Accepted |
| [0097](0097-local-coordination-gateway-composes-plane-workflows.md) | A local coordination gateway composes plane workflows | Accepted |
| [0098](0098-connection-trust-reuses-the-enrolled-key-oidc-waits.md) | Connection trust reuses the enrolled node key; OIDC waits for a real second tenant | Accepted |
| [0099](0099-a-claim-also-checks-for-a-live-twin-by-title.md) | A claim also checks for a live twin by title, not only by scope | Accepted |
| [0100](0100-repository-node-owns-one-non-exporting-signer.md) | The repository node owns one non-exporting signer | Accepted |
| [0101](0101-a-digest-is-compiled-not-hand-authored.md) | A digest is a compiled view of the graph, not a hand-authored document | Accepted |
| [0102](0102-context-is-compiled-not-assembled-by-hand.md) | Context is compiled into a bounded packet, not assembled by hand | Accepted |
| [0103](0103-the-mcp-client-is-packaged-once-not-reimplemented.md) | The MCP client is packaged once, not reimplemented per consumer | Accepted |
| [0104](0104-a-reference-consumer-tests-the-tool-surfaces-stability.md) | A reference consumer tests the tool surface's stability | Accepted |
| [0105](0105-bridge-is-the-server-version-of-the-vsix.md) | The Bridge is the server version of the VSIX | Accepted |
| [0106](0106-ackplane-closes-the-agentic-operating-loop.md) | Ackplane closes the agentic operating loop | Accepted |
| [0107](0107-registered-agents-accept-authenticated-control-directives.md) | Registered agents accept authenticated control directives | Accepted |
| [0108](0108-knowledge-rpcs-authenticate-with-operation-signing.md) | Knowledge RPCs authenticate with an operation-signing scheme mirroring claims | Accepted |
| [0109](0109-a-live-claim-may-consent-to-follow-its-amended-clause.md) | A live claim may consent to follow its amended clause | Accepted |
| [0110](0110-a-ledger-act-is-independently-verifiable-evidence.md) | A Lodestar ledger act is independently verifiable evidence | Accepted |
| [0111](0111-bridge-recovers-a-stranded-claim-as-a-tenant-scoped-administrative-action.md) | Bridge recovers a stranded claim as a tenant-scoped administrative action | Accepted |
| [0112](0112-bridge-list-reads-are-paginated-sortable-and-filterable.md) | Bridge list reads are paginated, sortable, and filterable server-side | Accepted |
| [0113](0113-the-industrial-knowledge-plane-is-evidence-backed-and-human-governed.md) | The Industrial knowledge plane is evidence-backed and human-governed | Accepted |
| [0114](0114-context-packets-are-bounded-attributed-and-reproducible.md) | Context packets are bounded, attributed, and reproducible | Accepted |
| [0115](0115-human-delegation-bounds-industrial-agent-autonomy.md) | Human delegation bounds Industrial agent autonomy | Accepted |
| [0116](0116-enrolled-supervisors-are-the-distributed-agent-runtime.md) | Enrolled supervisors are the distributed agent runtime | Accepted |
| [0117](0117-the-bridge-live-feed-is-a-replayable-operations-stream.md) | The Bridge live feed is a replayable operations stream | Accepted |
| [0118](0118-the-coverage-gate-provisions-postgres-for-ackplane.md) | The Coverage gate provisions Postgres for Ackplane's tests | Accepted |
| [0119](0119-industrial-administration-lifecycle-policy.md) | Industrial administration is receipted, scoped, and never a reset | Accepted |
| [0120](0120-industrial-work-domain-is-an-authoritative-task-projection.md) | Industrial Work is an authoritative task projection, not a mirrored board | Accepted |
| [0121](0121-industrial-design-preserves-immutable-history.md) | Industrial Design preserves immutable history, not a mutable current snapshot | Accepted |
| [0122](0122-enrollment-status-proof-distinguishes-unreachable-from-unrecognized-binding.md) | Enrollment-status proof distinguishes unreachable from unrecognized binding | Accepted |
| [0123](0123-bridge-exposes-a-first-industrial-design-mutation-slice.md) | Bridge exposes a first bounded Industrial Design mutation slice | Accepted |
| [0124](0124-bridge-static-pages-share-one-chrome-asset-not-a-copy-each.md) | Bridge static pages share one chrome asset, not a copy each | Accepted |
| [0125](0125-bridge-work-commands-are-principal-scoped-and-receipted.md) | Bridge Work commands are principal-scoped and receipted | Accepted |
| [0126](0126-the-bridge-proposes-constitution-amendments-only-a.md) | The Bridge proposes constitution amendments; only a repository activates them | Accepted |
| [0127](0127-agent-issued-shell-commands-are-auditable-evidence.md) | Agent-issued shell commands are auditable evidence, not just prose discipline | Accepted |
| [0128](0128-the-hardened-loopback-profile-is-the-verified-principal-for-self-hosted-administration.md) | The hardened loopback profile is the verified principal for self-hosted Industrial administration | Accepted |
| [0130](0130-adopt-worktree-consults-a-live-claim.md) | Adopting a worktree refuses a claim it would not be handed | Accepted |
| [0132](0132-a-bind-address-is-not-a-reachable-interface.md) | A bind address is not a reachable interface | Accepted |
| [0133](0133-a-shared-test-database-is-not-a-test-database.md) | A shared test database is not a test database | Accepted |
| [0134](0134-enrolled-signing-keys-authenticate-lifecycle-purge-confirmations.md) | Enrolled signing keys authenticate Lifecycle purge confirmations | Accepted |
| [0135](0135-a-directive-receipt-survives-a-dropped-connection.md) | A directive receipt survives a dropped connection | Accepted |
| [0136](0136-ackplane-gains-an-mcp-front-door-not-a-duplicated-storage-core.md) | Ackplane gains an MCP front door; it does not duplicate the local planes' storage core | Accepted |
| [0137](0137-ackplane-mcp-authenticates-by-borrowing-an-enrolled-node-key.md) | `ackplane-mcp` authenticates by borrowing an enrolled node's key | Accepted |
| [0138](0138-a-known-limitation-is-not-a-gap.md) | A known limitation is not a gap | Accepted |
| [0139](0139-ackplane-mcp-task-surface-scopes-to-existing-claim-and-read-authority.md) | `ackplane-mcp`'s task surface scopes to Ackplane's existing claim and read authority, not full Lodestar parity | Accepted |
| [0140](0140-a-pgvector-recall-store-scoped-to-projected-nodes.md) | A `pgvector` recall store scoped to `projected_nodes`, not the curated `knowledge` domain | Accepted |
| [0141](0141-ackplane-reports-its-own-supervisor-position.md) | Ackplane reports its own supervisor position, not the client's echo | Proposed |
| [0142](0142-loopback-verified-principal-extends-to-work-design-and-constitution.md) | The hardened loopback profile is also the verified principal for Work commands, Design mutations, and Constitution proposals | Accepted |
| [0144](0144-a-superseded-clause-records-who-retired-it.md) | A superseded clause records who retired it | Accepted |

## Writing a new ADR

1. Copy an existing file to `NNNN-short-title.md` (next number).
2. Fill in Context / Decision / Consequences.
3. Link it from the code or doc it constrains.

**Do not edit the table above by hand** — it is generated from the ADR files by
`scripts/adr-index.mjs`, and a pre-commit hook fails if it drifts. Run
`make adr-index` (or the script directly) after adding or restatusing an ADR.
Hand-maintaining it meant every branch appended a row to the same table, so
every merge conflicted on it; it also drifted, with ten of forty-five rows wrong
when the generator was introduced.

"Next number" is not obvious when several branches are in flight: your working
tree cannot see a sibling branch's unmerged ADR. A pre-commit hook
(`scripts/adr-number-guard.mjs`) checks the number against every ref and names
the first free one, so a collision costs seconds here instead of a renumber
across the file, its cross-links, and every commit message citing it. Run it
directly to pick a number before you start:

```bash
node scripts/adr-number-guard.mjs docs/adr/0042-my-decision.md
```
