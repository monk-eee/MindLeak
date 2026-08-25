//! JSON-Schema definitions for the executive tools (`task_create`,
//! `task_claim`, `task_transition`, `task_query`).

use serde_json::{json, Value};

pub(in crate::tools) fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "task_create",
            "description": "Create work serving a goal. With `title`: one task. `blocked_by` keeps it unclaimable until that predecessor completes aligned. `also_serves` declares additional goals this work serves, fixed for the task's life. Never refuses for existing work: `duplicates` names a live exact-title match under the goal; `prior_work` lists every task ever created under it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "goal_id": { "type": "string" },
                    "title": { "type": "string", "description": "The single task to create. Omit to decompose the goal into tasks instead." },
                    "acceptance": { "type": "string", "description": "What 'done' means." },
                    "blocked_by": { "type": "string", "description": "Optional predecessor task. The new task opens automatically only after that task completes aligned." },
                    "also_serves": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional additional goal ids this work legitimately serves. Declared here and fixed for the task's life; there is no verb that adds coverage later. A verdict that relied on one caps at needs_human, so declaring breadth buys a review, not a pass (ADR-0041)."
                    }
                },
                "required": ["goal_id"]
            }
        }),
        json!({
            "name": "task_claim",
            "description": "Ownership and the lease, chosen by `step`. `claim`: atomic claim + lease with optional advisory `paths`/MindLeak `symbols`; `won=true` only if this agent won, and a loss names why. Succeeds for ANY agent once the task is `open` or `claimed` with an expired lease — no `recover` needed for an ordinary lapsed lease. A won claim resolves active clauses against those paths (`scope_advice`; `review` names an uncovered goal to add via `also_serves`), reports `title_twin` naming another live task under this same goal sharing this exact title if one exists (ADR-0099), and returns the evidence window completion later validates against. `renew`: extends a still-live lease owned by this agent; after expiry only a fresh `claim` opens a new window. `reconnect_clause` (renew only, ADR-0109): reconnects this task's goal from a superseded clause onto its active same-slug successor, refused unless exactly one exists. `release`: hands the claim back to open, owner-guarded. `recover`: only for two cases `claim` cannot reach — a pre-ADR-0054 legacy owner string (rare today), or an early `paused`-task transfer with a named human reviewer (`expected_owner` and `reason` required; `human` is attributed, not authenticated, and must differ from both owners).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" },
                    "step": { "type": "string", "enum": ["claim", "renew", "release", "recover"], "description": "Which ownership act. Each names the further arguments it needs." },
                    "agent": { "type": "string", "description": "Optional when LODESTAR_AGENT is configured." },
                    "lease_secs": { "type": "integer", "default": 300, "description": "Lease length for claim, renew and recover." },
                    "paths": { "type": "array", "items": { "type": "string" }, "description": "claim: workspace-relative path globs this work expects to touch. Omit to preserve existing paths on re-claim; pass [] to clear them." },
                    "symbols": { "type": "array", "items": { "type": "string" }, "description": "claim: opaque MindLeak symbol ids this work expects to touch. Omit to preserve existing symbols on re-claim; pass [] to clear them." },
                    "also_serves": { "type": "array", "items": { "type": "string" }, "default": [], "description": "claim: further goal ids this work serves, for what advise reported over the files you are actually touching. Unions with what was declared at creation, so naming only what you just learned never drops what you knew. Refused once conformance has judged this task." },
                    "reconnect_clause": { "type": "boolean", "default": false, "description": "renew: also ask to reconnect this task's own live claim onto its clause's active same-slug successor (ADR-0109). Only the current owner's own live claim is eligible." },
                    "expected_owner": { "type": "string", "description": "recover: the exact current owner. A recovery that does not name who it is taking from is not a recovery." },
                    "reason": { "type": "string", "description": "recover: why ownership moved." },
                    "human": { "type": "string", "description": "recover: distinct human reviewer authorizing a paused-task transfer before the parking grace. An attributable declaration, not authentication." }
                },
                "required": ["task_id", "step"]
            }
        }),
        json!({
            "name": "task_transition",
            "description": "Move a task through the lifecycle, chosen by `to`. `complete`: consumes an authoritative check_conformance result for the same claim-bounded evidence — aligned completes, drift/uncertainty stay in_review, violation blocks; `learned` records durable knowledge for the next agent (omit if it taught nothing). `resolve`: human-accepts an in_review task to done, no conformance re-run, opening any blocked successor. `block`: marks nonterminal work blocked and clears any live claim; `reason` is the only way the former owner learns why. `reopen`: returns stranded work (in_review after drift/needs_human, or a manual hold with no predecessor gate) to open; refuses to bypass a handoff, disturb a live claim, or revive terminal work. `abandon`: permanently retires nonterminal work; needs `acknowledge_branch=true` if any recorded branch exists, since abandon is irreversible and cannot see whether it carries an open or merged pull request. `pause`/`resume`: the owner deliberately suspends and restarts, keeping owner and evidence window. `ask`: parks a claimed task with a durable question (`audience` addresses a peer; omit for a human); `answer` returns it to claimed with a fresh lease.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" },
                    "to": {
                        "type": "string",
                        "enum": ["complete", "resolve", "block", "reopen", "abandon", "pause", "resume", "ask", "answer"],
                        "description": "The transition to make. Each names the further arguments it needs and why."
                    },
                    "agent": { "type": "string", "description": "Optional when LODESTAR_AGENT is configured." },
                    "evidence": { "type": "object", "description": "complete: versioned ConformanceEvidence returned by MindLeak evidence_for." },
                    "check": { "type": "object", "description": "complete: the id/token reference returned by check_conformance, or the full legacy id/token/verdict/findings object." },
                    "learned": { "type": "string", "description": "complete: what this task taught that the next agent would otherwise rediscover. Omit when it taught nothing." },
                    "human": { "type": "string", "description": "resolve: non-empty reviewer label recorded in resolved_by. Attributed, not authenticated; must differ from the agent id under review." },
                    "blocked_by": { "type": "string", "description": "block: optional predecessor, which must be same-goal, acyclic, and part of a one-to-one handoff chain." },
                    "reason": { "type": "string", "description": "block and pause: why. Recorded as a durable note on the task's thread, readable by its former owner." },
                    "actor": { "type": "string", "default": "human", "description": "block: who blocked it." },
                    "question": { "type": "string", "description": "ask: the durable question that parks the task." },
                    "audience": { "type": "string", "description": "ask: agent id to address the question to. Omit to ask a human." },
                    "answer": { "type": "string", "description": "answer: the durable answer." },
                    "author": { "type": "string", "default": "human", "description": "answer: who answered." },
                    "lease_secs": { "type": "integer", "default": 300, "description": "resume and answer: length of the fresh lease." },
                    "acknowledge_branch": { "type": "boolean", "default": false, "description": "abandon: confirm no branch the task's history ever recorded (not only its current one) carries an open or merged pull request. Required whenever any such branch exists, since abandon is a one-way door and the ledger cannot see a pull request from here." }
                },
                "required": ["task_id", "to"]
            }
        }),
        json!({
            "name": "task_query",
            "description": "Every read over tasks, chosen by `view`. `board`: coordination snapshot with owner, status and lease. `doctor`: read-only diagnosis of the same board — identical titles under one goal, one title forked across several goals, and work blocked with no predecessor and no reason recorded. `rework`: over `since`, how many created tasks repeated an existing title, how many of those in the SAME SECOND as the task they repeat (a generator's signature, not an agent's), and the worst repeated titles; abandonment is reported beside it and not counted as rework. `next`: the oldest unblocked claimable task. `scope`: one task's declared advisory path/symbol scope — a planning hint, never a lock. `existing_work`: has this already been done, including finished and abandoned tasks. `overlap`: pre-flight check for live claims intersecting concrete `paths`/`symbols`, classified by the branches the two sessions declared at open_session — same_branch_collision, cross_branch_merge_risk, or undeclared (needs `session_id` to classify). `stalled`: every task not progressing, and what stalled it. `thread`: a task's durable append-only dialogue. `pending_questions`: addressed to you; `questions_for_a_human`: waiting on a person. `drafts`: proposed questions a task's owner could put to colliding peers. `claim_transfers`: the append-only ownership recovery history. All views are read-only and evidence-free.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "view": {
                        "type": "string",
                        "enum": ["board", "doctor", "rework", "next", "scope", "existing_work", "overlap", "stalled", "thread", "pending_questions", "questions_for_a_human", "drafts", "claim_transfers"],
                        "description": "Which question to ask. Each names the further arguments it needs."
                    },
                    "task_id": { "type": "string", "description": "scope, thread, drafts and claim_transfers: the task to read." },
                    "since": { "type": "integer", "default": 0, "description": "rework: unix second to measure from. 0 (default) is the whole ledger. Redundancy is always judged against the full history, so narrowing the window never makes a repeat of older work disappear." },
                    "include_terminal": { "type": "boolean", "default": true, "description": "board: include terminal done/abandoned tasks (default true); false returns only the live/actionable set." },
                    "detail": { "type": "boolean", "default": true, "description": "board: also include each task's scope, claim_window, receipt, and acceptance text; false omits them for a lean scan, dropping no task." },
                    "branch": { "type": "string", "description": "board: narrow to tasks recorded on exactly this branch, any status, independent of include_terminal." },
                    "goal_id": { "type": "string", "description": "existing_work: work already serving this goal." },
                    "paths": { "type": "array", "items": { "type": "string" }, "default": [], "description": "existing_work and overlap: concrete workspace-relative paths. Declared claim scopes are the glob side of the comparison." },
                    "symbols": { "type": "array", "items": { "type": "string" }, "default": [], "description": "overlap: opaque MindLeak symbol ids." },
                    "exclude_task_id": { "type": "string", "description": "overlap: optional current task to omit from results." },
                    "session_id": { "type": "string", "pattern": "^[0-9a-f]{32}$", "description": "overlap: optional session id from open_session. Supplying it classifies each overlap against the branch that session already declared; without it every signal is 'undeclared'. No branch argument is accepted: the branch is declared once per session, and a second place to state it could disagree with the first." }
                },
                "required": ["view"]
            }
        }),
    ]
}
