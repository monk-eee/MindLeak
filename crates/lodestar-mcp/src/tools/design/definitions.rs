//! JSON schemas for the four design verbs.

use serde_json::{json, Value};

use super::super::design_materialization::materialization_plan_schema;

pub(in crate::tools) fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "design_register",
            "description": "Put ADRs into the design ledger. One design (adr_path + title) registers it as proposed and tainted under ADR-0023: invisible to next_task and the executive board, present on the design board, id derived from the ADR path, attributed to the registering session. A batch (designs) instead reconciles structured repository ADR metadata idempotently — it never invokes a model, never creates goals or tasks, and existing human decisions and promotion state always win. Supply one shape or the other, not both.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "adr_path": { "type": "string", "description": "Path to the ADR, e.g. docs/adr/0023-....md. Registers a single design." },
                    "title": { "type": "string", "description": "Required with adr_path." },
                    "summary": { "type": "string", "description": "What the design decides; used later to decompose it into tasks." },
                    "designs": {
                        "type": "array",
                        "description": "Reconcile many at once from repository metadata.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "adr_path": { "type": "string" },
                                "title": { "type": "string" },
                                "summary": { "type": "string" },
                                "status": { "type": "string", "enum": ["proposed", "accepted", "rejected"] },
                                "proposed_by": { "type": "string" }
                            },
                            "required": ["adr_path", "title", "status"]
                        }
                    }
                }
            }
        }),
        json!({
            "name": "design_decide",
            "description": "Every attributed human act on a design. Supply exactly one of id or ids; ids is supported for defer, resume, reject, and retire so one reviewed act can update a backlog. decision=accept is the guarded acceptance (ADR-0023): the design becomes accepted with promotion state 'pending', it runs no code conformance and creates no tasks, and no agent may accept its own design — materialise the work with design_promote. decision=reject is durable and auditable, spawns no work, and requires a rationale. decision=defer (ADR-0077) parks a proposed design without changing its status and requires a reason; decision=resume reverses that explicit deferral. decision=retire (ADR-0042) says the record should not have existed — use it for an orphan row whose ADR was renamed or renumbered; retiring is not deleting. decision=supersede (ADR-0050) says an accepted decision was replaced and links its registered successor. decision=reopen (ADR-0047) repairs an imported status nobody decided. decision=attribute (ADR-0051) records who made a decision the ledger already asserts but attributes to nobody.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Design item id, e.g. design:0023-design-board-accept-bridge" },
                    "ids": { "type": "array", "items": { "type": "string" }, "minItems": 1, "uniqueItems": true, "description": "Batch targets for defer, resume, reject, or retire. Mutually exclusive with id." },
                    "decision": { "type": "string", "enum": ["accept", "reject", "defer", "resume", "retire", "supersede", "reopen", "attribute"] },
                    "human": { "type": "string", "description": "The person making the act. Required for every decision except reopen, which records none." },
                    "reason": { "type": "string", "description": "Required for reject, defer, resume, and retire." },
                    "superseded_by": { "type": "string", "description": "Required for supersede: the registered design that replaces this one." }
                },
                "required": ["decision"]
            }
        }),
        json!({
            "name": "design_promote",
            "description": "Materialise an accepted design into work, in reviewed steps. step=plan produces a read-only suggested create plan under one objective and never creates tasks — the caller must show and review it. step=materialize applies exactly one explicit human-reviewed create/link/no_work plan; idempotent retries return the same revision and never duplicate work. step=revise appends an attributed repair revision and replaces the current provenance projection, leaving prior plans and tasks durable; a non-empty rationale is required.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Accepted design item id." },
                    "step": { "type": "string", "enum": ["plan", "materialize", "revise"] },
                    "objective_goal_id": { "type": "string", "description": "Required for plan." },
                    "plan": materialization_plan_schema(),
                    "human": { "type": "string", "description": "Required for revise: the person recording the repair." }
                },
                "required": ["id", "step"]
            }
        }),
        json!({
            "name": "design_query",
            "description": "Read the design ledger. view=board lists actionable items: non-deferred proposed ADRs awaiting a human decision plus accepted designs awaiting or retrying promotion — distinct from the executive task board. view=ledger reads the durable record, optionally filtered by status, including historical and materialized items; retired records are omitted unless include_retired and deferred records are omitted unless include_deferred. view=promotion reads materialized provenance. view=history reads every immutable materialization and repair revision. view=actions reads the immutable attributed defer/resume/reject/retire history for one design. All read-only.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "view": { "type": "string", "enum": ["board", "ledger", "promotion", "history", "actions"] },
                    "id": { "type": "string", "description": "Required for promotion, history, and actions." },
                    "status": { "type": "string", "enum": ["proposed", "accepted", "rejected"], "description": "Filters ledger." },
                    "include_retired": { "type": "boolean", "description": "Include retired records in the ledger audit view. Defaults to false." },
                    "include_deferred": { "type": "boolean", "description": "Include deferred proposals in the ledger audit view. Defaults to false." }
                },
                "required": ["view"]
            }
        }),
    ]
}
