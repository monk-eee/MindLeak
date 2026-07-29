//! The policy-pack half of the constitution vocabulary (ADR-0059).
//!
//! Split from `super` because these are two clusters sharing a name, not one:
//! the constitution verbs move a *version* through its lifecycle, while these
//! move a *pack's clauses* through review. Keeping them in one file put it past
//! the module-length clause, and the clause was right — the two halves are read
//! and changed independently.
//!
//! A pack never becomes law without an explicit adopt, tailor, or reject.
//! Everything here produces proposals.

use lodestar_core::model::Consequence;
use lodestar_core::{ConstitutionPack, Lodestar, PackClause, PackClauseDisposition};
use serde_json::{json, Value};

use super::{bool_arg, ok, opt_str, req_str};

pub(super) fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "policy_pack_register",
            "description": "Bring a policy pack into the review pipeline (ADR-0059). `action` names the transition: `register` validates and registers one immutable pack version (idempotent for the same id/version/digest, refused for different content under an existing version); `propose` creates durable review proposals for every undecided clause in a registered pack, returning needs_human for declared conflicts and never re-proposing a rejected clause; `common_core` registers and proposes the five review-first Common Core principles; `fleet_delivery` registers and proposes fleet-delivery v2. Everything here produces proposals — a pack never becomes law without an explicit adopt, tailor, or reject.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["register", "propose", "common_core", "fleet_delivery"] },
                    "pack": { "type": "object", "description": "Required for register: ConstitutionPack including its canonical SHA-256 digest." },
                    "pack_id": { "type": "string", "description": "Required for propose." },
                    "version": { "type": "string", "description": "Required for propose." },
                    "constitution_version": { "type": "string", "description": "Optional explicit draft/active constitution id; defaults to the active version." }
                },
                "required": ["action"]
            }
        }),
        json!({
            "name": "policy_pack_decide",
            "description": "Record a decision about a proposed clause (ADR-0059). `action` names it: `clause` attributes one human review disposition (adopted, tailored, or rejected) to a pack-clause proposal — adoption copies a self-contained local clause plus immutable source provenance, and conflicts or pack upgrades still require explicit later resolution; `contract` completes an adopted clause's enforcement contract so it can govern rather than merely advise. A disposition is a human act and is attributed as one.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["clause", "contract"] },
                    "proposal_id": { "type": "string", "description": "Required for clause." },
                    "disposition": { "type": "string", "enum": ["adopted", "tailored", "rejected"], "description": "Required for clause." },
                    "tailored_clause": { "type": "object", "description": "Required only for tailored; must preserve the source clause key." },
                    "reason": { "type": "string", "description": "Required for rejection; recommended for tailoring." },
                    "clause_id": { "type": "string", "description": "Required for contract." },
                    "scope": { "type": "string", "description": "Required for contract." },
                    "evidence_contract": { "type": "string", "description": "Required for contract." },
                    "consequence": { "type": "string", "description": "Optional for contract." },
                    "waiver_authority": { "type": "string", "description": "Optional for contract." },
                    "agent": { "type": "string", "description": "Injected from the registered session." }
                },
                "required": ["action", "agent"]
            }
        }),
        json!({
            "name": "policy_pack_query",
            "description": "Read the policy-pack review record (ADR-0059). `action` names what: `proposals` lists clause proposals for one pack/version and constitution context; `provenance` reports where an adopted clause came from, which is the only way to tell a locally authored clause from one inherited from a pack. Read-only and evidence-free.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["proposals", "provenance"] },
                    "pack_id": { "type": "string", "description": "Required for proposals." },
                    "version": { "type": "string", "description": "Required for proposals." },
                    "constitution_version": { "type": "string" },
                    "include_decided": { "type": "boolean", "default": false },
                    "goal_id": { "type": "string", "description": "Required for provenance." }
                },
                "required": ["action"]
            }
        }),
        json!({
            "name": "register_policy_pack",
            "description": "DEPRECATED — call `policy_pack_register` with action=register. Accepted for one more minor version, then removed (ADR-0059). Validate and register one immutable policy-pack version. Same id/version/digest is idempotent; different content under an existing version is refused.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pack": { "type": "object", "description": "ConstitutionPack including its canonical SHA-256 digest." }
                },
                "required": ["pack"]
            }
        }),
        json!({
            "name": "propose_policy_pack",
            "description": "DEPRECATED — call `policy_pack_register` with action=propose. Accepted for one more minor version, then removed (ADR-0059). Create durable review proposals for every undecided clause in an immutable policy pack. Declared conflicts return needs_human; rejected clauses are not re-proposed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pack_id": { "type": "string" },
                    "version": { "type": "string" },
                    "constitution_version": { "type": "string", "description": "Optional explicit draft/active constitution id; defaults to the active version." }
                },
                "required": ["pack_id", "version"]
            }
        }),
        json!({
            "name": "propose_common_core",
            "description": "DEPRECATED — call `policy_pack_register` with action=common_core. Accepted for one more minor version, then removed (ADR-0059). Register and propose the five review-first Common Core principles (evidence, intent, safety, proportionality, evolution). They are proposals, never implicit law.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "propose_fleet_delivery",
            "description": "DEPRECATED — call `policy_pack_register` with action=fleet_delivery. Accepted for one more minor version, then removed (ADR-0059). Register and propose fleet-delivery v2: protected-branch review, one publishing owner per task branch, isolated worktrees, commit identity, scoped commits, branch freshness, and topology honesty. Proposals only — every clause still needs an explicit adopt, tailor, or reject before it governs anything.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "list_pack_proposals",
            "description": "DEPRECATED — call `policy_pack_query` with action=proposals. Accepted for one more minor version, then removed (ADR-0059). List policy-pack clause proposals for one pack/version and constitution context.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pack_id": { "type": "string" },
                    "version": { "type": "string" },
                    "constitution_version": { "type": "string" },
                    "include_decided": { "type": "boolean", "default": false }
                },
                "required": ["pack_id", "version"]
            }
        }),
        json!({
            "name": "review_pack_clause",
            "description": "DEPRECATED — call `policy_pack_decide` with action=clause. Accepted for one more minor version, then removed (ADR-0059). Attribute one human review disposition (adopted, tailored, or rejected) to a pack-clause proposal. Adoption copies a self-contained local clause plus immutable source provenance; conflicts and pack upgrades require explicit later resolution.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "proposal_id": { "type": "string" },
                    "disposition": { "type": "string", "enum": ["adopted", "tailored", "rejected"] },
                    "tailored_clause": { "type": "object", "description": "Required only for tailored; must preserve the source clause key." },
                    "reason": { "type": "string", "description": "Required for rejection; recommended for tailoring." },
                    "agent": { "type": "string", "description": "Injected from the registered session." }
                },
                "required": ["proposal_id", "disposition", "agent"]
            }
        }),
        json!({
            "name": "pack_clause_provenance",
            "description": "Resolve an adopted local goal back to its immutable source pack id, version, digest, key, and original clause.",
            "inputSchema": {
                "type": "object",
                "properties": { "goal_id": { "type": "string" } },
                "required": ["goal_id"]
            }
        }),
        json!({
            "name": "complete_clause_contract",
            "description": "Give a clause the enforcement contract it needs to drive a verdict: scope, evidence contract, consequence, and waiver policy. Until this is done a clause is review-only — migration deliberately invents none of these fields, so a rule never silently acquires the power to block, but the default is sticky and a project can run for a long time unable to reach a hard verdict about anything. Refuses a clause on an active version: hardening what governs people mid-flight is an amendment (propose_amendment, complete the contract on the draft, then amend_constitution).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clause_id": { "type": "string" },
                    "scope": { "type": "string", "description": "What the clause governs: an artifact:/symbol: id or prefix**, or a workflow: token for a procedural rule." },
                    "evidence_contract": { "type": "string", "description": "What evidence a check must supply to decide this clause." },
                    "consequence": { "type": "string", "enum": ["advise", "review", "block"], "description": "What the clause asks for. Bounded at resolution by the power of whatever control backs it (ADR-0034)." },
                    "waivable": { "type": "boolean", "default": false, "description": "Whether a bounded exception may be granted (SPEC-CONSTITUTION §9)." },
                    "waiver_authority": { "type": "string", "description": "Who may approve an exception. An unwaivable clause cannot name one." }
                },
                "required": ["clause_id", "scope", "evidence_contract"]
            }
        }),
    ]
}

pub(super) fn register(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let pack: ConstitutionPack = serde_json::from_value(
        args.get("pack")
            .cloned()
            .ok_or_else(|| "missing required object arg: pack".to_string())?,
    )
    .map_err(|error| format!("invalid policy pack: {error}"))?;
    ok(&engine
        .register_policy_pack(&pack)
        .map_err(|error| error.to_string())?)
}

pub(super) fn propose(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    ok(&engine
        .propose_policy_pack(
            req_str(args, "pack_id")?,
            req_str(args, "version")?,
            opt_str(args, "constitution_version").as_deref(),
        )
        .map_err(|error| error.to_string())?)
}

pub(super) fn proposals(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    ok(&engine
        .policy_pack_proposals(
            req_str(args, "pack_id")?,
            req_str(args, "version")?,
            opt_str(args, "constitution_version").as_deref(),
            bool_arg(args, "include_decided", false),
        )
        .map_err(|error| error.to_string())?)
}

pub(super) fn provenance(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    ok(&engine
        .pack_clause_provenance(req_str(args, "goal_id")?)
        .map_err(|error| error.to_string())?)
}

pub(super) fn review_clause(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let disposition = PackClauseDisposition::from_tag(req_str(args, "disposition")?)
        .ok_or_else(|| "disposition must be adopted, tailored, or rejected".to_string())?;
    let tailored: Option<PackClause> = args
        .get("tailored_clause")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| format!("invalid tailored clause: {error}"))?;
    ok(&engine
        .review_pack_clause(
            req_str(args, "proposal_id")?,
            disposition,
            tailored.as_ref(),
            req_str(args, "agent")?,
            opt_str(args, "reason").as_deref(),
        )
        .map_err(|error| error.to_string())?)
}

pub(super) fn complete_contract(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let consequence = match opt_str(args, "consequence").as_deref() {
        None => None,
        Some(tag) => {
            Some(Consequence::from_tag(tag).ok_or_else(|| format!("unknown consequence: {tag}"))?)
        }
    };
    let authority = opt_str(args, "waiver_authority");
    let clause = engine
        .complete_clause_contract(
            req_str(args, "clause_id")?,
            req_str(args, "scope")?,
            req_str(args, "evidence_contract")?,
            consequence,
            bool_arg(args, "waivable", false),
            authority.as_deref(),
        )
        .map_err(|error| error.to_string())?;
    ok(&json!({
        "clause_id": clause.id,
        "scope": clause.scope,
        "evidence_contract": clause.evidence_contract,
        "consequence": clause.consequence.map(|c| c.as_str()),
        "waivable": clause.waivable,
        "waiver_authority": clause.waiver_authority,
        "status": clause.status.as_str(),
        "note": "a clause resolves at min(consequence, control ceiling); register a control or it stays advisory (ADR-0034)",
    }))
}
