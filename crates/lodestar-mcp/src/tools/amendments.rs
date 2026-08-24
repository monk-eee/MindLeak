//! Amendment and pack-upgrade tool definitions and dispatch
//! (SPEC-CONSTITUTION §9).

use super::{ok, opt_str, req_str};
use lodestar_core::{GoalKind, Lodestar};
use serde_json::{json, Value};

pub(super) fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "propose_amendment",
            "description": "Begin changing adopted policy: draft the next constitutional version, carrying every active clause forward so the draft starts as the current policy. Edit the draft, then amend_constitution promotes it. Starting from a copy is what keeps the eventual diff readable — only what you actually change appears in it. Refuses when no constitution is active (that is an activation) or a draft is already open.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "draft_clause",
            "description": "Author a NEW rule into an open amendment draft, then give it a contract with complete_clause_contract before amend_constitution promotes it. This is how policy grows: define_goal states a rule that is live the moment it is written, and complete_clause_contract refuses to harden a live rule (that is what an amendment is for), so without this the clause most needing an enforcement contract was the one clause that could never be given one. The clause takes effect only if the draft is promoted, and appears in constitution_diff as 'added' for the reviewer. Use this rather than minting a policy pack for a rule this project wrote itself — a pack records immutable upstream provenance, which would be a fabricated source.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "draft_id": { "type": "string", "description": "The draft opened by propose_amendment." },
                    "kind": { "type": "string", "enum": ["objective", "constraint", "invariant"] },
                    "title": { "type": "string" },
                    "statement": { "type": "string", "description": "The normative text: what must hold." }
                },
                "required": ["draft_id", "kind", "title", "statement"]
            }
        }),
        json!({
            "name": "amend_constitution",
            "description": "Promote a reviewed amendment draft, retiring the version it replaces and recording an attributed rationale plus an explicit clause diff. The old version and its clauses are superseded, never deleted, so prior conformance records keep naming the policy they were judged under. `approved_by` names who accepted the change and must differ from the calling agent: an agent may approve an amendment, just not its own, which is what lets the audit history tell a reviewed adoption from an agent changing policy alone. Attributed, not authenticated (ADR-0071). Refuses an amendment that changes nothing, one that leaves no clauses at all, and one carrying an undecided clause proposal.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "draft_id": { "type": "string" },
                    "approved_by": { "type": "string", "description": "Who accepted this change. Must differ from the calling agent; another agent is allowed, approving your own is not." },
                    "rationale": { "type": "string", "description": "Why the rule is changing — the thing a reader will want most in a year." }
                },
                "required": ["draft_id", "approved_by", "rationale"]
            }
        }),
        json!({
            "name": "constitution_diff",
            "description": "The clause-level difference between two constitutional versions, changing nothing — what an amendment would do. Clauses match on slug, so a restated rule reads as changed rather than as a removal plus an addition, and a clause that only hardens its scope or consequence still shows up.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from_version": { "type": "string" },
                    "to_version": { "type": "string" }
                },
                "required": ["from_version", "to_version"]
            }
        }),
        json!({
            "name": "amendments",
            "description": "The amendment history, newest first — how policy got to where it is, with each rationale and stored diff.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "plan_pack_upgrade",
            "description": "Compare a newer pack version against what this project actually adopted from it. A proposal, never an upgrade: an upstream version change can never alter active local policy, so this is a pure read that produces the argument for amending. Clauses you tailored locally are flagged, because accepting an upstream change to one would silently discard a deliberate local decision.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pack_id": { "type": "string" },
                    "to_version": { "type": "string" }
                },
                "required": ["pack_id", "to_version"]
            }
        }),
    ]
}

pub(super) fn dispatch(
    engine: &Lodestar,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    match name {
        "propose_amendment" => Some(propose_amendment(engine, args)),
        "draft_clause" => Some(draft_clause(engine, args)),
        "amend_constitution" => Some(amend_constitution(engine, args)),
        "constitution_diff" => Some(constitution_diff(engine, args)),
        "amendments" => Some(amendments(engine)),
        "plan_pack_upgrade" => Some(plan_pack_upgrade(engine, args)),
        _ => None,
    }
}

fn propose_amendment(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let version = engine
        .propose_amendment(opt_str(args, "agent").as_deref())
        .map_err(|e| e.to_string())?;
    ok(&json!({
        "draft_id": version.id,
        "version": version.version,
        "status": version.status.as_str(),
        "note": "carried the active clauses forward; edit the draft, then amend_constitution",
    }))
}

fn draft_clause(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let kind = GoalKind::from_tag(req_str(args, "kind")?).ok_or_else(|| {
        format!(
            "invalid kind: {}",
            req_str(args, "kind").unwrap_or_default()
        )
    })?;
    let clause = engine
        .draft_clause(
            req_str(args, "draft_id")?,
            kind,
            req_str(args, "title")?,
            req_str(args, "statement")?,
        )
        .map_err(|e| e.to_string())?;
    ok(&json!({
        "clause_id": clause.id,
        "slug": clause.slug,
        "status": clause.status.as_str(),
        "constitution_version": clause.constitution_version,
        "note": "authored into the draft; give it a contract with complete_clause_contract, \
                 then amend_constitution promotes it",
    }))
}

fn amend_constitution(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let amendment = engine
        .amend_constitution(
            req_str(args, "draft_id")?,
            req_str(args, "agent")?,
            req_str(args, "approved_by")?,
            req_str(args, "rationale")?,
        )
        .map_err(|e| e.to_string())?;
    ok(&json!({
        "amendment_id": amendment.id,
        "from_version": amendment.from_version,
        "to_version": amendment.to_version,
        "amended_by": amendment.amended_by,
        "approved_by": amendment.approved_by,
        "diff": amendment.diff,
    }))
}

fn constitution_diff(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let diff = engine
        .constitution_diff(req_str(args, "from_version")?, req_str(args, "to_version")?)
        .map_err(|e| e.to_string())?;
    ok(&json!({ "diff": diff }))
}

fn amendments(engine: &Lodestar) -> Result<Value, String> {
    let history = engine.amendments().map_err(|e| e.to_string())?;
    ok(&json!({ "amendments": history }))
}

fn plan_pack_upgrade(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let plan = engine
        .plan_pack_upgrade(req_str(args, "pack_id")?, req_str(args, "to_version")?)
        .map_err(|e| e.to_string())?;
    ok(&json!(plan))
}

#[cfg(test)]
mod tests {
    // Generated by AI (UnitTest MCP)
    use super::*;
    use lodestar_core::PackClauseDisposition;

    fn engine() -> Lodestar {
        Lodestar::open_in_memory().unwrap()
    }

    /// A governed project with the Common Core adopted, mirroring
    /// lodestar-core's own `facade::amendments::tests::governed` helper:
    /// `propose_amendment`/`amend_constitution` both require an active
    /// constitution to already exist.
    fn governed(e: &Lodestar) -> String {
        let proposal = e
            .propose_constitution(&["README.md".to_string()], Some("monk-eee"))
            .unwrap();
        for clause in proposal.common_core.proposals {
            e.review_pack_clause(
                &clause.id,
                PackClauseDisposition::Adopted,
                None,
                "monk-eee",
                Some("Adopted as proposed"),
            )
            .unwrap();
        }
        e.activate_constitution(&proposal.version.id, "monk-eee")
            .unwrap();
        proposal.version.id
    }

    fn body(result: &Value) -> Value {
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap())
            .expect("ok() emits valid JSON text")
    }

    fn open_draft(e: &Lodestar) -> String {
        let result = dispatch(e, "propose_amendment", &json!({ "agent": "monk-eee" }))
            .expect("tool is dispatched")
            .expect("a governed project can open a draft");
        body(&result)["draft_id"].as_str().unwrap().to_string()
    }

    #[test]
    fn unknown_tools_are_not_claimed_by_this_module() {
        assert!(dispatch(&engine(), "grant_waiver", &json!({})).is_none());
    }

    #[test]
    fn propose_amendment_dispatch_refuses_when_no_constitution_is_active() {
        let engine = engine();
        let err = dispatch(&engine, "propose_amendment", &json!({}))
            .expect("tool is dispatched")
            .expect_err("a fresh engine has no active constitution to amend");
        assert_eq!(
            err,
            "invalid: no constitution is active; propose_constitution adopts a first one"
        );
    }

    #[test]
    fn propose_amendment_dispatch_opens_a_draft_carrying_the_active_clauses_forward() {
        let engine = engine();
        let active = governed(&engine);

        let result = dispatch(
            &engine,
            "propose_amendment",
            &json!({ "agent": "monk-eee" }),
        )
        .expect("tool is dispatched")
        .expect("a governed project can open a draft");
        let draft = body(&result);
        let draft_id = draft["draft_id"].as_str().unwrap().to_string();
        assert_ne!(draft_id, active);
        assert_eq!(draft["status"], "draft");
        assert_eq!(
            draft["note"],
            "carried the active clauses forward; edit the draft, then amend_constitution"
        );

        // A fresh draft starts identical to the version it was opened from.
        let diff_result = dispatch(
            &engine,
            "constitution_diff",
            &json!({ "from_version": active, "to_version": draft_id }),
        )
        .expect("tool is dispatched")
        .expect("both versions exist");
        assert_eq!(body(&diff_result)["diff"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn propose_amendment_dispatch_refuses_a_second_open_draft() {
        let engine = engine();
        governed(&engine);
        open_draft(&engine);

        let err = dispatch(&engine, "propose_amendment", &json!({}))
            .expect("tool is dispatched")
            .expect_err("a draft is already open");
        assert!(
            err.contains("is already drafted and awaiting review"),
            "{err}"
        );
    }

    #[test]
    fn draft_clause_dispatch_rejects_an_invalid_kind() {
        let engine = engine();
        governed(&engine);
        let draft_id = open_draft(&engine);

        let err = dispatch(
            &engine,
            "draft_clause",
            &json!({
                "draft_id": draft_id,
                "kind": "not-a-real-kind",
                "title": "New rule",
                "statement": "Something must hold.",
            }),
        )
        .expect("tool is dispatched")
        .expect_err("not-a-real-kind is not a GoalKind");
        assert_eq!(err, "invalid kind: not-a-real-kind");
    }

    #[test]
    fn draft_clause_dispatch_authors_a_new_clause_into_the_open_draft() {
        let engine = engine();
        governed(&engine);
        let draft_id = open_draft(&engine);

        let result = dispatch(
            &engine,
            "draft_clause",
            &json!({
                "draft_id": draft_id,
                "kind": "constraint",
                "title": "New rule",
                "statement": "Something must hold.",
            }),
        )
        .expect("tool is dispatched")
        .expect("the draft is open and the kind is valid");
        let clause = body(&result);
        assert_eq!(clause["status"], "draft");
        assert_eq!(clause["constitution_version"], draft_id);
        assert!(!clause["clause_id"].as_str().unwrap().is_empty());
        assert!(!clause["slug"].as_str().unwrap().is_empty());
        assert_eq!(
            clause["note"],
            "authored into the draft; give it a contract with complete_clause_contract, \
                 then amend_constitution promotes it"
        );

        // The new clause is what makes this draft differ from the active version.
        let diff_result = dispatch(
            &engine,
            "constitution_diff",
            &json!({ "from_version": "constitution:v1", "to_version": draft_id }),
        )
        .expect("tool is dispatched")
        .expect("both versions exist");
        let diff = body(&diff_result)["diff"].as_array().unwrap().clone();
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0]["change"], "added");
    }

    #[test]
    fn amend_constitution_dispatch_refuses_self_approval() {
        let engine = engine();
        governed(&engine);
        let draft_id = open_draft(&engine);
        dispatch(
            &engine,
            "draft_clause",
            &json!({
                "draft_id": draft_id,
                "kind": "constraint",
                "title": "New rule",
                "statement": "Something must hold.",
            }),
        )
        .unwrap()
        .unwrap();

        let err = dispatch(
            &engine,
            "amend_constitution",
            &json!({
                "draft_id": draft_id,
                "agent": "monk-eee",
                "approved_by": "monk-eee",
                "rationale": "Needed a new rule.",
            }),
        )
        .expect("tool is dispatched")
        .expect_err("the proposer cannot also approve their own amendment");
        assert!(err.contains("cannot also approve it"), "{err}");
    }

    #[test]
    fn amend_constitution_dispatch_promotes_a_draft_and_records_an_attributed_diff() {
        let engine = engine();
        let active = governed(&engine);
        let draft_id = open_draft(&engine);
        dispatch(
            &engine,
            "draft_clause",
            &json!({
                "draft_id": draft_id,
                "kind": "constraint",
                "title": "New rule",
                "statement": "Something must hold.",
            }),
        )
        .unwrap()
        .unwrap();

        let result = dispatch(
            &engine,
            "amend_constitution",
            &json!({
                "draft_id": draft_id,
                "agent": "monk-eee",
                "approved_by": "reviewer",
                "rationale": "Needed a new rule.",
            }),
        )
        .expect("tool is dispatched")
        .expect("a reviewed draft with a real diff can be promoted");
        let amendment = body(&result);
        assert_eq!(amendment["from_version"], active);
        assert_eq!(amendment["to_version"], draft_id);
        assert_eq!(amendment["amended_by"], "monk-eee");
        assert_eq!(amendment["approved_by"], "reviewer");
        assert_eq!(amendment["diff"].as_array().unwrap().len(), 1);

        let history_result = dispatch(&engine, "amendments", &json!({}))
            .expect("tool is dispatched")
            .expect("the history read never fails");
        let history = body(&history_result)["amendments"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["to_version"], draft_id);
    }

    #[test]
    fn constitution_diff_dispatch_reports_no_difference_between_a_version_and_itself() {
        let engine = engine();
        let active = governed(&engine);

        let result = dispatch(
            &engine,
            "constitution_diff",
            &json!({ "from_version": active, "to_version": active }),
        )
        .expect("tool is dispatched")
        .expect("comparing a version to itself never fails");
        assert_eq!(body(&result)["diff"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn amendments_dispatch_reports_no_history_on_a_fresh_engine() {
        let result = dispatch(&engine(), "amendments", &json!({}))
            .expect("tool is dispatched")
            .expect("an empty history is not an error");
        assert_eq!(body(&result)["amendments"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn plan_pack_upgrade_dispatch_reports_not_found_for_an_unknown_pack_version() {
        let err = dispatch(
            &engine(),
            "plan_pack_upgrade",
            &json!({ "pack_id": "no-such-pack", "to_version": "v99" }),
        )
        .expect("tool is dispatched")
        .expect_err("no such pack version was ever registered");
        assert!(err.contains("no-such-pack"), "{err}");
    }

    #[test]
    fn each_dispatch_function_reports_its_own_missing_required_argument() {
        let engine = engine();
        assert_eq!(
            dispatch(&engine, "draft_clause", &json!({}))
                .unwrap()
                .unwrap_err(),
            "missing required string arg: kind"
        );
        assert_eq!(
            dispatch(&engine, "amend_constitution", &json!({}))
                .unwrap()
                .unwrap_err(),
            "missing required string arg: draft_id"
        );
        assert_eq!(
            dispatch(&engine, "constitution_diff", &json!({}))
                .unwrap()
                .unwrap_err(),
            "missing required string arg: from_version"
        );
        assert_eq!(
            dispatch(&engine, "plan_pack_upgrade", &json!({}))
                .unwrap()
                .unwrap_err(),
            "missing required string arg: pack_id"
        );
    }
}
