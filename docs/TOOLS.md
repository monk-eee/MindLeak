# MCP tool reference

Every tool both MindLeak servers expose. This lives outside the README so
that the front door stays a router: the tables below are a reference to look
things up in, not something to read.

New here? Start with **[QUICKSTART.md](QUICKSTART.md)** to get running, then
**[USAGE.md](USAGE.md)** for how an agent actually uses these in a session.

Model-backed results use one additive contract: `model_call.source` is `model`
or `fallback`; a fallback also carries `fallback_reason` as `unreachable`,
`timeout`, `bad_json`, or `misconfigured`. Calling either plane's
`storage_status` with `include_model_health=true` performs one probe and adds
`model_health` (`configured`, `reachable`, `responds_json`, URL/model, and an
optional failure reason/detail). Without that argument, no model call is made
and the field is omitted.

---

## Memory Plane tools (`mindleak-mcp`)


| Tool | Purpose |
|---|---|
| `open_session` | Register a client-minted 128-bit session id and return its stable cross-plane agent identity; required before identity-bearing calls (ADR-0030). Optionally declares the session's `branch` / `head_sha` / `base` / `dirty` working context (ADR-0035). Also carries what the agent should know on pickup: `stale_build` when this binary is behind its checkout, `waiting_on_you` and `paused_by_you` for work addressed to this agent, and `awaiting_a_human` — work that completed into `in_review` and can only be moved by a person. That last one is fleet-level rather than addressed, because completing clears the owner and a human has no agent id (ADR-0046), so it belongs to nobody; the agent is told because the agent is the only thing the human talks to. Each field appears only when it has something to say. |
| `graph_multi_hop_query` | Traverse N hops from a seed node/phrase, decay-filtered. |
| `get_impact_radius` | Blast radius of editing a file/symbol. |
| `check_overlap` | Read-only, decay-aware footprint of other agents on concrete paths / symbol ids; combine with Lodestar's same-named claim check (ADR-0024). |
| `record_architectural_decision` | Persist a decision as a linked intent node. |
| `ingest_execution` | Command + exit code → execution/modified/failed_on edges. |
| `ingest_commit` | Commit → intent node + refactored edges + rationale. |
| `ingest_file` | File → artifact + extracted symbols (`contains`); `structural_only` refreshes deterministic structure without recording agent attention. |
| `forget_file` | Deleted/renamed file → reap its artifact, symbols, and their edges. |
| `reconcile_workspace` | Forget artifacts outside the workspace set and report stale/missing extractor snapshots; `report_only` performs the same inspection without deleting anything. |
| `boost_entity` | Record node focus for recency views without rewriting evidence. |
| `graph_snapshot` | Subgraph for visualization. |
| `prune_graph` | Surface near-expiry proven signal for consolidation, then purge decayed noise and unreferenced stubs. |
| `graph_stats` | Node / active-edge counts. |
| `export_graph` | Complete active graph JSON with fully derived edge weights (not a backup). |
| `backup_database` | Create an integrity-checked online SQLite backup of the memory plane. |
| `reset_database` | Clear regenerable memory only with the exact `RESET MINDLEAK` token. |
| `consolidate_session` | Optional: compress raw logs into one intent node through the configured OpenAI-compatible endpoint; the default is local, while hosted endpoints use the configured API key. Successful output includes `model_call.source="model"`, while typed failures name why no result was produced. |
| `consolidate_signal` | Optional: consolidate queued proven signal, persist provenance links, then acknowledge raw evidence; successful output carries the same `model_call` marker. |
| `promotion_candidates` | Aggregate expiring proven signal into subject-level candidates for Lodestar `promote_signals` — the deterministic, model-free promotion pass that closes the learned-knowledge loop (ADR-0022). |
| `list_agents` | Roster of agents + their active observation counts (attribution). |
| `working_set` | Current agent's bounded, ranked attentional focus (derived from active observations; default cap 7). |
| `evidence_for` | Bounded, provenance-bearing evidence bundle from an agent's attributed executions/commits in a work window (ADR-0009). |
| `index` | Optional: embed nodes lacking a current vector through the configured OpenAI-compatible `/v1/embeddings` endpoint (ADR-0008). |
| `recall` | Optional: nearest node ids by cosine similarity — entry points to *seed* `graph_multi_hop_query`. |
| `telemetry_snapshot` | Observability record (ADR-0010): per-tool lifetime call/error counts, latency, current health (whether each tool's most recent call failed), and recent invocations from the durable audit trail. |
| `storage_status` | Resolved repository id, graph database path, storage origin, legacy migration source, and whether migration ran (ADR-0038); `include_model_health=true` adds one on-demand consolidation-model probe (ADR-0079). |

---

## Intent Plane tools (Lodestar)


A second, **durable** MCP server ([`lodestar-mcp`](crates/lodestar-mcp)) — the
intent, coordination, and proof plane that keeps parallel agents aligned to
shared goals and constraints. Register it alongside `mindleak-mcp`; both servers
derive the same per-clone repository id and user-local directory, so isolated
worktrees share one intent plane and one memory graph by default.

> **Evidence is the proof.** Completion here isn't a claim an agent makes — it's
> proof it must produce. `task_transition` (`to="complete"`) accepts only a provenance-bearing evidence
> bundle that a separate `check_conformance` scores against the goal's code, bounded
> by the live claim and attributed to the acting agent. The durable
> `conformance_history` chain is the **only** trustworthy record that a fleet did
> the right thing — narration is not proof — and `export_evidence` makes it portable
> for review, a CI gate, and audit.

> **The default profile is the common path.** Every agent loads `tools/list`
> before its first question, so an unspent minute of governance authoring is a
> tax paid in every session (ADR-0059 rule 2). By default `lodestar-mcp`
> advertises only the tools an agent uses to find, claim, do, prove and hand off
> work, plus the ones it reads to know what governs it — 17 tools, ~4,513 tokens,
> against 67 tools and ~13,757 tokens for the whole surface. The specialist
> machinery below — the constitution and amendments, policy packs, waivers,
> ratchets and controls, the design board, goal↔code binding, knowledge
> maintenance, and database admin — is not advertised by default but stays fully
> reachable: dispatch is unchanged, so a specialist tool called by name still
> runs. Set `LODESTAR_TOOL_PROFILE=full` to advertise everything. Measure any
> time with `node scripts/measure-tool-surface.mjs`.

| Tool | Purpose |
|---|---|
| `constitution_define` | Write or rewrite intent — `action`: `goal` · `supersede` · `bind` · `unbind`. |
| `constitution_decide` | Move a version through its lifecycle — `action`: `propose` · `activate`. |
| `constitution_query` | Read policy and what it governs — `action`: `active` · `status` · `governing` · `for_task` · `export`. |
| `policy_pack_register` / `policy_pack_decide` / `policy_pack_query` | The policy-pack half of the same vocabulary: register or propose a pack, attribute a clause disposition or complete its contract, and read the review record. |
| `define_goal` / `supersede_goal` | Write/version the constitution (objective · constraint · invariant). |
| `get_constitution` | The authoritative intent to read **before acting**. |
| `constitution_status` | Whether this project has an active constitution, a draft awaiting review, or none at all, with the version and its clause count. |
| `propose_constitution` | Classify supplied repository paths into cited project facts, record them as a draft's provenance, and propose the Common Core. Deterministic, model-free, and never activates. |
| `activate_constitution` | Promote a reviewed draft to the governing constitution in one atomic transaction. Refuses undecided clauses, an empty draft, a non-draft, or a second active version. |
| `register_policy_pack` / `propose_policy_pack` | Validate and register one immutable pack version, then create durable clause-review proposals for a draft or active constitution. |
| `propose_common_core` / `list_pack_proposals` | Propose the five review-first Common Core principles through the same pack path, and inspect undecided or historical dispositions. |
| `propose_fleet_delivery` | Propose fleet-delivery v2: protected-branch review, one publishing owner per task branch, isolated worktrees, commit identity, scoped commits, freshness, and topology honesty. |
| `review_pack_clause` / `pack_clause_provenance` | Session-attributed adopt/tailor/reject; adoption copies a self-contained local clause and preserves immutable source pack provenance. |
| `complete_clause_contract` | Give a clause the scope, evidence contract, consequence, and waiver policy it needs to drive a verdict. Until this is done a clause is review-only — migration invents none of those fields, so a rule never silently gains the power to block. Refuses an active clause: hardening what already governs people is an amendment. |
| `register_control` | Bind a versioned mechanism to a clause. Without one the clause is an orphan and resolves at `advise` whatever it declares, because a rule with no mechanism behind it is a preference (ADR-0034). Declare the power the mechanism honestly has; `observed` and `advisory` cap at `review`. |
| `retire_control` | Stand a control down when it is superseded or was registered under the wrong id. Attributed to the calling session and permanent — retiring a control is the one act that reduces what a clause can enforce without changing a word of the clause. Retirement is not deletion: the control keeps recording what it enforced, so observations naming it resolve as `unknown` rather than disappearing. Without it a misregistered control is permanent, because its version can never move backwards. |
| `advise` | **Ask before acting** (ADR-0029): given the `artifact:`/`symbol:` ids you intend to change, returns the governing clauses + a proportional disposition (advise / review / block / needs_human). Evidence-free, records nothing, needs no model, never gates a claim. |
| `link_goal_to_artifact` | Bind a goal to the MindLeak `artifact:`/`symbol:` nodes that realise it — source, or equally an ADR, doc, benchmark or build script (ADR-0060). |
| `unlink_goal_from_artifact` / `governing_goals` | Prune a stale goal↔artifact binding, and audit which goals govern a node — keeps conformance honest. |
| `governing_for_task` | The clauses governing a task's linked scope — what the Work view surfaces on in-progress work (ADR-0029). |
| `register_ratchet` / `accept_ratchet_baseline` | Bind a metric that must not regress to one clause, then accept the reviewed baseline it compares against. A ratchet never moves its own baseline, and reports `unknown` until one exists. |
| `observe_ratchet` / `clause_controls` | Report a measurement and resolve it through its clause, capped at `review` by the ratchet's observed power (ADR-0034); and list the mechanisms behind a clause with the force each actually has. |
| `grant_waiver` / `revoke_waiver` | The reviewable form of `--no-verify`: a scoped, expiring, attributed exception to one clause. Refuses an unwaivable clause, the wrong approver, and any expiry not in the future — a permanent exception is an amendment. Revocation is immediate for future checks and never retroactive. |
| `clause_waivers` / `active_waivers` | Every exception ever granted against a clause (a rule waived repeatedly is a rule that wants amending), and everything not being enforced right now, soonest to expire first. |
| `propose_amendment` / `amend_constitution` | Change adopted policy explicitly: draft the next version carrying every active clause forward, then promote it with an attributed rationale and a clause diff. The old version is superseded, never deleted, so prior conformance records keep naming the policy they were judged under. |
| `draft_clause` | Author a **new** rule into an open amendment draft, then give it a contract with `complete_clause_contract` before promoting. This is how policy grows: `define_goal` writes a rule that is live immediately, and `complete_clause_contract` refuses to harden a live rule, so a genuinely new clause had no route to an enforcement contract. It takes effect only if the draft is promoted, and shows in `constitution_diff` as `added`. Use this rather than minting a policy pack for a rule this project wrote itself — a pack records immutable upstream provenance. |
| `constitution_diff` / `amendments` | What an amendment would do, and how policy got to where it is. Clauses match on slug, so a restated rule reads as `changed` — and a clause that only hardens its scope or consequence still shows up. |
| `plan_pack_upgrade` | Compare a newer pack version against what this project adopted from it. A proposal, never an upgrade — upstream can never alter active local policy. Locally tailored clauses are flagged, because accepting an upstream change to one would silently discard a deliberate decision. |
| `export_constitution` | Render the constitution to committed-friendly markdown. |
| `task_create` | Add claimable work. With `title`, one task: `blocked_by` creates a progressive handoff, and `also_serves` declares up front the additional goals genuinely cross-cutting work serves (ADR-0041). Without `title`, the goal is decomposed into tasks instead. A created task reports and names the work already serving that goal (`prior_work`) — it never refuses, because a second task against one goal is often legitimate (ADR-0015). |
| `task_claim` | Ownership and the lease, chosen by `step`. `claim` is the **atomic claim + lease** with optional advisory path globs / symbol ids, returning the evidence window it opened and, on a loss, which of the knowable reasons it was rather than a bare `false`. On re-claim, omitting `paths` or `symbols` preserves that field; supplying an array replaces it, and `[]` deliberately clears it. A won scoped claim resolves active clauses against those paths: `governing` includes matched goals, and `scope_advice` reports `review` with an actionable `also_serves` correction when a matched goal is not covered. It also takes `also_serves`, because goals bind to files and the governing set is usually learned while working: a held claim can declare further goals it serves, unioned with what creation declared, until conformance has judged the task (ADR-0074). `renew` is the live heartbeat; after expiry a same-owner re-claim keeps the evidence window and records the lapse (ADR-0048). `release` returns the work. `recover` guardedly takes an expired compatible legacy claim into the registered session, or transfers a paused task before its grace with a named reviewer. Claiming and renewing carry `waiting_on_you` when a peer has addressed a question to this agent (ADR-0046), so a question arrives on a call you already make rather than one you must remember. |
| `task_transition` | Every lifecycle move, chosen by `to`. `complete` consumes the exact authoritative `check_conformance` result (owner-guarded): aligned completes, uncertainty reviews, violation blocks. `resolve` human-accepts an `in_review` task to `done` with no code-conformance re-run — the task-level mirror of `design_decide(decision="accept")` — under a reviewer label that is attributed, not authenticated (ADR-0071), and must differ from the agent under review. `block` takes a task off whoever held it, so it records a `reason` they can read. `reopen` returns a stranded task (in review, or a manual hold) to claimable `open`. `abandon` retires open/review/blocked or expired-claim work to durable `abandoned`, leaving live and parked ownership protected; a task that recorded a branch refuses unless `acknowledge_branch=true`, because abandon is irreversible and cannot see whether that branch already carries an open or merged pull request. `pause` / `resume` are the owner deliberately parking and restarting, keeping the claim and evidence window. `ask` parks a claimed task in `needs_input` with a durable question — set `audience` to address a peer agent instead of a human — and `answer` resumes it under the same owner with a fresh lease. |
| `task_query` | Every read over tasks, chosen by `view`. `board` is the who-owns-what-and-where snapshot including advisory scope, claim window, receipt and whether the lease is actually live; the VS Code Work view defaults to live/actionable work, while `include_terminal=true` returns durable history. `next` suggests the next unblocked claimable task. `existing_work` answers "has this already been done?" — **including finished and abandoned work**, which is exactly what `board` hides and what makes duplicate work invisible; path matching reuses the `overlap` glob comparison so the two never drift. `overlap` answers the different question "who is touching this file right now", advisory only and combined by the caller with MindLeak's footprint result (ADR-0024). `scope` reads one claim declaration. `thread` is the durable append-only dialogue for a task. `pending_questions` is what is addressed to you and `questions_for_a_human` what is waiting on a person — necessarily separate, because a human has no agent id, so a query matching an id can never return one (ADR-0046). `drafts` turns a scope collision the ledger already holds into an addressed, ready-to-send question (ADR-0055). `stalled` reports every task that is not progressing and the fact that stalled it, reporting how long without judging whether that is too long. `claim_transfers` is the append-only prior-owner/window audit. |
| `fleet_view` | Read-only: who is working where, from the context sessions declared on `open_session` — branch, head, base, how far behind that base each said it was, and whether live sessions disagree about their base. Also who is waiting on whom, any wait cycle where agents can only be unstuck from outside, and any stale one-way wait whose addressee has gone quiet — no live claim and no session since the question, so it will otherwise sit until the parking grace (ADR-0046). Advisory and capped at `review`; undeclared values report `unknown` rather than being guessed (ADR-0035, ADR-0044). |
| `design_register` | Register one ADR (`adr_path`, `title`), or idempotently import structured repository ADR metadata by passing a `designs` batch instead. Reconciliation creates no tasks. Passing both shapes at once is refused rather than guessed. |
| `design_decide` | Every attributed human act on a registered design, chosen by `decision`: `accept`, `reject`, `defer` / `resume` (ADR-0077), `retire` (ADR-0042), `supersede` (ADR-0050), `reopen` (ADR-0047), `attribute` (ADR-0051). Supply exactly one `id`, or an `ids` batch for `defer`, `resume`, `reject`, or `retire`; a batch shares one human and reason and returns every affected row. Deferral parks an active proposal without changing its `proposed` status, and resume is its explicit inverse. Each decision states which arguments it requires and why, and no decision runs code conformance or permits self-acceptance; accepting or rejecting aligns the ADR file's declared status. The refusals the separate names used to encode survive as argument validation: `attribute` never overwrites a `decided_by` already present, and `reopen` defers to materialisation, refusing a row whose promotion has already created work. Retiring is not deleting, and superseding keeps the row `accepted` while linking a registered successor. |
| `design_promote` | The promotion sequence, chosen by `step`: `plan` previews task drafts without writes, `materialize` atomically applies one reviewed create/link/no-work plan across the named objectives, and `revise` appends an attributed, rationale-bearing repair. Prior plans and tasks remain durable. |
| `design_query` | Every read over the design ledger, chosen by `view`: `board` (non-deferred proposed decisions and accepted designs awaiting promotion), `ledger` (the durable record, optionally filtered by `status`; retired rows require `include_retired` and deferred rows require `include_deferred`), `promotion` (the current task/objective projection), `history` (immutable materialization revisions), and `actions` (the immutable attributed defer/resume/reject/retire history for one design). |
| `check_conformance` | Persist and return `{ id, token, verdict, findings }` for exact checked completion. |
| `conformance_history` | Resolve a task's durable evidence chain — the recorded bundle, verdict, and stable id per check. |
| `certification_status` | The qualified certification status a subject holds (ADR-0090). Verification is the capability; certification is the status it produces, and it is never a bare badge: the status carries its subject and commit, the policy version judged against, the evidence bundle behind it, the date, and which clauses were evaluated and which were not. It runs no new judgement — the verdict is the deterministic conformance record the task already closed on. `certified`, `not_certified` with its reason, `waived` with its expiry and remediation, `needs_human`, `uncertifiable` where no constitution has been adopted, and `stale` where the subject moved past its evidence; none of them renders as certified. Pass `at_commit` to ask about a revision — the server never reads Git, so staleness is judged against what you declare. It never asserts compliance with an external framework: a status certifies conformance to the clauses it names and nothing more. |
| `export_evidence` | Render a task's conformance chain as a committed, verifiable **proof-of-work** artifact — the proof leaves the local ledger for review, CI, and audit (ADR-0031). |
| `export_conformance_manifest` | Render every task's latest verdict as one manifest for the CI conformance gate (`scripts/conformance-gate.mjs`), so a build can fail on unresolved drift rather than on a human remembering to look. |
| `consolidate` / `record_knowledge` | Gated promotion of learned regularities. `record_knowledge` optionally takes an attributed `/memories/repo/...` or `/memories/session/...` `source_ref`: exact repeats reconfirm one lesson and edits supersede the source's prior record (ADR-0081). |
| `promote_signals` | Promotion bridge (ADR-0022): batch-feed MindLeak `promotion_candidates` into the gated consolidator; deterministic, model-optional. |
| `active_knowledge` / `reconfirm_knowledge` / `prune_knowledge` | Durable-but-revalidated knowledge. `active_knowledge` takes an optional `query` that ranks by meaning (ADR-0080), or `source_ref` to resolve one current agent-memory lesson (ADR-0081). When no embedder is reachable semantic search falls back to substring matching and `match_mode` says so. A search warms the index, so early searches may report `ranked_by_meaning` below `count` — the remainder is still in weight order. |
| `retire_knowledge` | Withdraw a lesson that is wrong or replaced, or detach one agent-memory `source_ref`. Source detachment retires the lesson only after its final source disappears. Every ending is attributed and reasoned; the record stays readable. |
| `lodestar_stats` | Goal / task / knowledge counts. |
| `storage_status` | Resolved repository id, intent database path, storage origin, legacy migration source, and whether migration ran (ADR-0038); `include_model_health=true` adds one on-demand Lodestar-model probe (ADR-0079). |
| `backup_database` | Create an integrity-checked online SQLite backup of the intent plane. |
| `reset_database` | Clear durable intent only with the exact `RESET LODESTAR` token. |

Design: [docs/SPEC-INTENT.md](docs/SPEC-INTENT.md) ·
[docs/SPEC-CONSTITUTION.md](docs/SPEC-CONSTITUTION.md) ·
[ADR-0004](docs/adr/0004-intent-plane-spec-brain.md) ·
[ADR-0005](docs/adr/0005-signal-weighted-decay.md) ·
[ADR-0012](docs/adr/0012-derived-signal-evidence.md) ·
[ADR-0026](docs/adr/0026-constitutional-policy-over-mechanistic-ratchets.md).

Backup, upgrade, rollback, export, reset, and retention guidance:
[docs/DATA-LIFECYCLE.md](docs/DATA-LIFECYCLE.md).

The VS Code sidebar includes a separate **Design Board**. It synchronizes
structured ADR metadata, keeps human review separate from executable task
coordination, supports attributed accept/reject/defer/resume acts, and reviews
explicit create/link/no-work plans before materialization. Proposed rows appear
before pending promotions; a header reports how many await decision, the tree
initially caps its tail at 20 rows, and explicit expand/show-deferred controls
reveal the rest without moving or deciding anything in the ledger. Materialized
rows expose persisted provenance/history and an attributed repair action.
