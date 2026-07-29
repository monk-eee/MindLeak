# MCP tool reference

Every tool both MindLeak servers expose. This lives outside the README so
that the front door stays a router: the tables below are a reference to look
things up in, not something to read.

New here? Start with **[QUICKSTART.md](QUICKSTART.md)** to get running, then
**[USAGE.md](USAGE.md)** for how an agent actually uses these in a session.

---

## Memory Plane tools (`mindleak-mcp`)


| Tool | Purpose |
|---|---|
| `open_session` | Register a client-minted 128-bit session id and return its stable cross-plane agent identity; required before identity-bearing calls (ADR-0030). Optionally declares the session's `branch` / `head_sha` / `base` / `dirty` working context (ADR-0035). |
| `graph_multi_hop_query` | Traverse N hops from a seed node/phrase, decay-filtered. |
| `get_impact_radius` | Blast radius of editing a file/symbol. |
| `check_overlap` | Read-only, decay-aware footprint of other agents on concrete paths / symbol ids; combine with Lodestar's same-named claim check (ADR-0024). |
| `record_architectural_decision` | Persist a decision as a linked intent node. |
| `ingest_execution` | Command + exit code → execution/modified/failed_on edges. |
| `ingest_commit` | Commit → intent node + refactored edges + rationale. |
| `ingest_file` | File → artifact + extracted symbols (`contains`). |
| `forget_file` | Deleted/renamed file → reap its artifact, symbols, and their edges. |
| `reconcile_workspace` | Forget artifacts for files no longer in the workspace set (bulk cleanup). |
| `boost_entity` | Record node focus for recency views without rewriting evidence. |
| `graph_snapshot` | Subgraph for visualization. |
| `prune_graph` | Surface near-expiry proven signal for consolidation, then purge decayed noise and unreferenced stubs. |
| `graph_stats` | Node / active-edge counts. |
| `export_graph` | Complete active graph JSON with fully derived edge weights (not a backup). |
| `backup_database` | Create an integrity-checked online SQLite backup of the memory plane. |
| `reset_database` | Clear regenerable memory only with the exact `RESET MINDLEAK` token. |
| `consolidate_session` | Optional: compress raw logs into one intent node via a local Ollama model. |
| `consolidate_signal` | Optional: consolidate queued proven signal, persist provenance links, then acknowledge raw evidence. |
| `promotion_candidates` | Aggregate expiring proven signal into subject-level candidates for Lodestar `promote_signals` — the deterministic, model-free promotion pass that closes the learned-knowledge loop (ADR-0022). |
| `list_agents` | Roster of agents + their active observation counts (attribution). |
| `working_set` | Current agent's bounded, ranked attentional focus (derived from active observations; default cap 7). |
| `evidence_for` | Bounded, provenance-bearing evidence bundle from an agent's attributed executions/commits in a work window (ADR-0009). |
| `index` | Optional: embed nodes lacking a current vector via a local `/v1/embeddings` server (ADR-0008). |
| `recall` | Optional: nearest node ids by cosine similarity — entry points to *seed* `graph_multi_hop_query`. |
| `telemetry_snapshot` | Observability record (ADR-0010): per-tool lifetime call/error counts, latency, current health (whether each tool's most recent call failed), and recent invocations from the durable audit trail. |
| `storage_status` | Resolved repository id, graph database path, storage origin, legacy migration source, and whether migration ran (ADR-0038). |

---

## Intent Plane tools (Lodestar)


A second, **durable** MCP server ([`lodestar-mcp`](crates/lodestar-mcp)) — the
"spec brain" that keeps parallel agents aligned to shared intent instead of
diluting it. Register it alongside `mindleak-mcp`; both servers derive the same
per-clone repository id and user-local directory, so isolated worktrees share
one intent plane and one memory graph by default.

> **Evidence is the proof.** Completion here isn't a claim an agent makes — it's
> proof it must produce. `complete_task` accepts only a provenance-bearing evidence
> bundle that a separate `check_conformance` scores against the goal's code, bounded
> by the live claim and attributed to the acting agent. The durable
> `conformance_history` chain is the **only** trustworthy record that a fleet did
> the right thing — narration is not proof — and `export_evidence` makes it portable
> for review, a CI gate, and audit.

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
| `create_task` / `decompose_goal` | Add claimable work; `create_task(blocked_by=...)` creates a progressive handoff, and `create_task(also_serves=[...])` declares up front the additional goals genuinely cross-cutting work serves (ADR-0041). A created task reports and names the work already serving that goal (`prior_work`) — it never refuses, because a second task against one goal is often legitimate (ADR-0015). |
| `existing_work` | Has this already been done? Returns the tasks already serving a goal, or already declaring any of these paths in their scope — **including finished and abandoned work**, which is exactly what `board` hides and what makes duplicate work invisible. Path matching reuses `check_overlap`'s glob comparison, so the two never drift. Distinct from `check_overlap`: that answers "who is touching this file right now", this answers "has someone already solved this". |
| `next_task` | Suggest the next unblocked, claimable task. |
| `claim_task` / `renew_lease` | **Atomic claim + lease** with optional advisory path globs / symbol ids — renewal is a live heartbeat; after expiry, a same-owner re-claim keeps the evidence window and records the lapse (ADR-0048). Both carry `waiting_on_you` when a peer has addressed a question to this agent (ADR-0046), so a question arrives on a call you already make rather than one you must remember. |
| `recover_claim` / `claim_transfer_history` | Guardedly recover an expired compatible legacy claim into the registered session and inspect the append-only prior-owner/window audit. |
| `task_scope` / `check_overlap` | Read one claim declaration or find live claims intersecting concrete paths / symbol ids; advisory only, and combined by the caller with MindLeak's footprint result (ADR-0024). |
| `complete_task` | Consume the exact authoritative `check_conformance` result (owner-guarded); aligned completes, uncertainty reviews, violation blocks. |
| `release_task` / `block_task` | Return or block work. Blocking takes a task off whoever held it, so it records a `reason` they can read. |
| `reopen_task` | Return a stranded task (in review, or a manual hold) to claimable `open`. |
| `abandon_task` | Retire open/review/blocked or expired-claim work to durable `abandoned`; live and parked ownership stays protected. |
| `resolve_task` | Human-accept an `in_review` task (a `drift`/`needs_human` completion) to `done` with no code-conformance re-run — the task-level mirror of `design_decide(decision="accept")`. Requires a reviewer identity and refuses self-resolution by the reviewed agent. |
| `ask_question` / `answer` | Park a claimed task in `needs_input` with a durable question; an `answer` resumes it under the same owner with a fresh lease. Set `audience` to address a peer agent instead of a human. |
| `pending_questions` | Unanswered questions addressed to you. A read over the durable threads, not a queue — nothing is delivered or consumed, so reading cannot lose one. |
| `questions_for_a_human` | Everything waiting on a **person**: each parked task's question, who asked, and how long it has gone unanswered, rendered as a readable inbox. Necessarily separate from `pending_questions`, because a human has no agent id — "addressed at a human" is the *absence* of an audience (ADR-0046), so an id match can never find one. Waiting time is reported, never judged. |
| `draft_questions` | Turn a scope collision the ledger already holds into an addressed, ready-to-send question for the peer who holds the other claim (ADR-0055). Read-only and evidence-free: it proposes, and `ask_question` remains the only thing that sends. The collision is deterministic; only the phrasing is model-assisted, falling back to a template when no local model is reachable, and each draft reports `drafted_by`. It never decides who should win. |
| `pause_task` / `resume_task` | Owner deliberately parks (`paused`) and resumes work, keeping the claim and evidence window. Pausing records an optional `reason`. |
| `task_qa` | The durable, append-only dialogue thread for a task: questions, answers, and notes explaining why a state change parked or blocked it. |
| `board` | Who-owns-what-and-where snapshot including advisory scope; the VS Code Work view defaults to live/actionable work, while `include_terminal=true` returns durable history. |
| `stalled_work` | Every task that is not progressing and the fact that stalled it: lapsed leases, work awaiting a human, work awaiting a peer agent, deadlocked waits, blocks behind something no agent will advance, blocks naming a task that is not on the board, and deliberately paused work. Read-only, evidence-free, and deliberately threshold-free — it reports how long a stall has been true and leaves "is that too long?" to a person. |
| `fleet_view` | Read-only: who is working where, from the context sessions declared on `open_session` — branch, head, base, how far behind that base each said it was, and whether live sessions disagree about their base. Also who is waiting on whom, and any wait cycle where agents can only be unstuck from outside (ADR-0046). Advisory and capped at `review`; undeclared values report `unknown` rather than being guessed (ADR-0035, ADR-0044). |
| `design_register` | Register one ADR (`adr_path`, `title`), or idempotently import structured repository ADR metadata by passing a `designs` batch instead. Reconciliation creates no tasks. Passing both shapes at once is refused rather than guessed. |
| `design_decide` | Every attributed human act on a registered design, chosen by `decision`: `accept`, `reject`, `retire` (ADR-0042), `supersede` (ADR-0050), `reopen` (ADR-0047), `attribute` (ADR-0051). Each decision states which arguments it requires and why, and no decision runs code conformance or permits self-acceptance; accepting or rejecting aligns the ADR file's declared status. The refusals the separate names used to encode survive as argument validation: `attribute` never overwrites a `decided_by` already present, and `reopen` defers to materialisation, refusing a row whose promotion has already created work — so the two continue to partition the undecided rows instead of overlapping. Retiring is not deleting (the row keeps its id, path, decision, and history, and simply leaves the working board), and superseding is neither: the row stays `accepted` and gains a link to a successor that must already be registered. |
| `design_promote` | The promotion sequence, chosen by `step`: `plan` previews task drafts without writes, `materialize` atomically applies one reviewed create/link/no-work plan across the named objectives, and `revise` appends an attributed, rationale-bearing repair. Prior plans and tasks remain durable. |
| `design_query` | Every read over the design ledger, chosen by `view`: `board` (proposed decisions and accepted designs awaiting promotion), `ledger` (the durable record, optionally filtered by `status`, with retired rows omitted unless `include_retired` so the board shows live decisions while the audit trail stays complete), `promotion` (the current task/objective projection), and `history` (the complete immutable review history). |
| `check_conformance` | Persist and return `{ id, token, verdict, findings }` for exact checked completion. |
| `conformance_history` | Resolve a task's durable evidence chain — the recorded bundle, verdict, and stable id per check. |
| `export_evidence` | Render a task's conformance chain as a committed, verifiable **proof-of-work** artifact — the proof leaves the local ledger for review, CI, and audit (ADR-0031). |
| `export_conformance_manifest` | Render every task's latest verdict as one manifest for the CI conformance gate (`scripts/conformance-gate.mjs`), so a build can fail on unresolved drift rather than on a human remembering to look. |
| `consolidate` / `record_knowledge` | Gated promotion of learned regularities. |
| `promote_signals` | Promotion bridge (ADR-0022): batch-feed MindLeak `promotion_candidates` into the gated consolidator; deterministic, model-optional. |
| `active_knowledge` / `reconfirm_knowledge` / `prune_knowledge` | Durable-but-revalidated knowledge. |
| `lodestar_stats` | Goal / task / knowledge counts. |
| `storage_status` | Resolved repository id, intent database path, storage origin, legacy migration source, and whether migration ran (ADR-0038). |
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
coordination, supports attributed accept/reject decisions, and reviews explicit
create/link/no-work plans before materialization. Materialized rows expose
persisted provenance/history and an attributed repair action.
