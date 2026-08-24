//! Waiver tool definitions and dispatch (SPEC-CONSTITUTION §9).

use super::{i64_arg, ok, opt_str, req_str};
use lodestar_core::waiver::WaiverRequest;
use lodestar_core::Lodestar;
use serde_json::{json, Value};

pub(super) fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "grant_waiver",
            "description": "Grant a scoped, expiring, attributed exception to one clause — the reviewable form of the thing an agent would otherwise do with --no-verify. Approved by the calling session, so an agent cannot approve an exception to a clause that names a human authority. Refuses an unwaivable clause, a wrong approver, and any expiry that is not in the future: a permanent exception is an amendment, not a waiver.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clause_id": { "type": "string" },
                    "scope": { "type": "string", "description": "What the exception covers: an artifact:/symbol:/workflow: token, or a prefix ending ** for everything beneath it." },
                    "reason": { "type": "string", "description": "Why the exception is justified — read at renewal time, so write it for someone else." },
                    "expires_at": { "type": "integer", "description": "Unix seconds. Required and must be in the future." },
                    "remediation_task_id": { "type": "string", "description": "The work that makes the exception unnecessary." }
                },
                "required": ["clause_id", "scope", "reason", "expires_at"]
            }
        }),
        json!({
            "name": "revoke_waiver",
            "description": "Withdraw a waiver. Immediate for future checks and never retroactive: prior conformance records keep the verdict they were given under the policy in force at the time. The row is kept, not deleted — the exception happened.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "waiver_id": { "type": "string" },
                    "reason": { "type": "string" }
                },
                "required": ["waiver_id", "reason"]
            }
        }),
        json!({
            "name": "clause_waivers",
            "description": "Every waiver ever granted against one clause, including lapsed and revoked ones. How often a rule has been excepted is usually the more useful question than what is excepted right now — a clause waived repeatedly is a clause that wants amending.",
            "inputSchema": {
                "type": "object",
                "properties": { "clause_id": { "type": "string" } },
                "required": ["clause_id"]
            }
        }),
        json!({
            "name": "active_waivers",
            "description": "Every exception currently in force, soonest to expire first — what is not being enforced right now, and until when.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
    ]
}

pub(super) fn dispatch(
    engine: &Lodestar,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    match name {
        "grant_waiver" => Some(grant_waiver(engine, args)),
        "revoke_waiver" => Some(revoke_waiver(engine, args)),
        "clause_waivers" => Some(clause_waivers(engine, args)),
        "active_waivers" => Some(active_waivers(engine)),
        _ => None,
    }
}

fn grant_waiver(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let granted = engine
        .grant_waiver(&WaiverRequest {
            clause_id: req_str(args, "clause_id")?.to_string(),
            scope: req_str(args, "scope")?.to_string(),
            reason: req_str(args, "reason")?.to_string(),
            approved_by: req_str(args, "agent")?.to_string(),
            expires_at: i64_arg(args, "expires_at", 0),
            remediation_task_id: opt_str(args, "remediation_task_id"),
        })
        .map_err(|e| e.to_string())?;
    ok(&json!({
        "waiver_id": granted.id,
        "clause_id": granted.clause_id,
        "scope": granted.scope,
        "approved_by": granted.approved_by,
        "expires_at": granted.expires_at,
        "remediation_task_id": granted.remediation_task_id,
        "constitution_version": granted.constitution_version,
    }))
}

fn revoke_waiver(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let revoked = engine
        .revoke_waiver(
            req_str(args, "waiver_id")?,
            req_str(args, "agent")?,
            req_str(args, "reason")?,
        )
        .map_err(|e| e.to_string())?;
    ok(&json!({
        "waiver_id": revoked.id,
        "status": revoked.status.as_str(),
        "revoked_by": revoked.revoked_by,
        "revoked_at": revoked.revoked_at,
    }))
}

fn clause_waivers(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let waivers = engine
        .clause_waivers(req_str(args, "clause_id")?)
        .map_err(|e| e.to_string())?;
    ok(&json!({ "waivers": waivers }))
}

fn active_waivers(engine: &Lodestar) -> Result<Value, String> {
    let waivers = engine.live_waivers().map_err(|e| e.to_string())?;
    ok(&json!({ "waivers": waivers }))
}

#[cfg(test)]
mod tests {
    // Generated by AI (UnitTest MCP)
    use super::*;
    use lodestar_core::model::GoalKind;
    use lodestar_core::PackClauseDisposition;

    fn engine() -> Lodestar {
        Lodestar::open_in_memory().unwrap()
    }

    fn body(result: &Value) -> Value {
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap())
            .expect("ok() emits valid JSON text")
    }

    /// A governed project with the Common Core adopted, mirroring
    /// lodestar-core's own `facade::amendments::tests::governed` helper.
    fn governed(e: &Lodestar) {
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
    }

    /// A governed project with one additional, already-active clause that
    /// explicitly declares itself waivable -- migration invents none of the
    /// enforcement fields for the Common Core, so `governed()` alone never
    /// produces a clause `grant_waiver` will accept.
    fn governed_with_waivable_clause(e: &Lodestar) -> String {
        let proposal = e
            .propose_constitution(&["README.md".to_string()], Some("monk-eee"))
            .unwrap();
        let draft_id = proposal.version.id.clone();
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
        let clause = e
            .draft_clause(
                &draft_id,
                GoalKind::Constraint,
                "Waivable rule",
                "Something must hold, but an exception may be granted.",
            )
            .unwrap();
        e.complete_clause_contract(&clause.id, "artifact:crates/**", "tests", None, true, None)
            .unwrap();
        e.activate_constitution(&draft_id, "monk-eee").unwrap();
        clause.id
    }

    #[test]
    fn unknown_tools_are_not_claimed_by_this_module() {
        assert!(dispatch(&engine(), "propose_amendment", &json!({})).is_none());
    }

    #[test]
    fn grant_waiver_dispatch_refuses_a_clause_that_does_not_declare_itself_waivable() {
        let engine = engine();
        governed(&engine);
        let clause_id = engine.get_constitution().unwrap()[0].id.clone();

        let err = dispatch(
            &engine,
            "grant_waiver",
            &json!({
                "clause_id": clause_id,
                "scope": "artifact:crates/**",
                "reason": "Needed for a deadline.",
                "expires_at": 4_000_000_000i64,
                "agent": "monk-eee",
            }),
        )
        .expect("tool is dispatched")
        .expect_err("a Common Core clause is not waivable by default");
        assert!(err.contains("declares itself unwaivable"), "{err}");
    }

    #[test]
    fn grant_waiver_dispatch_refuses_a_past_expiry() {
        let engine = engine();
        let clause_id = governed_with_waivable_clause(&engine);

        let err = dispatch(
            &engine,
            "grant_waiver",
            &json!({
                "clause_id": clause_id,
                "scope": "artifact:crates/**",
                "reason": "Needed for a deadline.",
                "expires_at": 1,
                "agent": "monk-eee",
            }),
        )
        .expect("tool is dispatched")
        .expect_err("an expiry in the past is a permanent exception, not a waiver");
        assert!(err.contains("must expire in the future"), "{err}");
    }

    #[test]
    fn grant_waiver_dispatch_grants_and_reports_a_live_waiver() {
        let engine = engine();
        let clause_id = governed_with_waivable_clause(&engine);

        let result = dispatch(
            &engine,
            "grant_waiver",
            &json!({
                "clause_id": clause_id,
                "scope": "artifact:crates/**",
                "reason": "Needed for a deadline.",
                "expires_at": 4_000_000_000i64,
                "remediation_task_id": null,
                "agent": "monk-eee",
            }),
        )
        .expect("tool is dispatched")
        .expect("a waivable clause with a future expiry grants cleanly");
        let granted = body(&result);
        assert!(granted["waiver_id"]
            .as_str()
            .unwrap()
            .starts_with("waiver:"));
        assert_eq!(granted["clause_id"], clause_id);
        assert_eq!(granted["scope"], "artifact:crates/**");
        assert_eq!(granted["approved_by"], "monk-eee");
        assert_eq!(granted["expires_at"], 4_000_000_000i64);
        assert!(granted["remediation_task_id"].is_null());
        assert!(!granted["constitution_version"].as_str().unwrap().is_empty());

        let active_result = dispatch(&engine, "active_waivers", &json!({}))
            .expect("tool is dispatched")
            .expect("the live read never fails");
        let active = body(&active_result)["waivers"].as_array().unwrap().clone();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0]["id"], granted["waiver_id"]);
    }

    #[test]
    fn revoke_waiver_dispatch_withdraws_an_active_waiver_and_it_leaves_the_live_set() {
        let engine = engine();
        let clause_id = governed_with_waivable_clause(&engine);
        let granted = body(
            &dispatch(
                &engine,
                "grant_waiver",
                &json!({
                    "clause_id": clause_id,
                    "scope": "artifact:crates/**",
                    "reason": "Needed for a deadline.",
                    "expires_at": 4_000_000_000i64,
                    "agent": "monk-eee",
                }),
            )
            .unwrap()
            .unwrap(),
        );
        let waiver_id = granted["waiver_id"].as_str().unwrap().to_string();

        let result = dispatch(
            &engine,
            "revoke_waiver",
            &json!({ "waiver_id": waiver_id, "agent": "reviewer", "reason": "No longer needed." }),
        )
        .expect("tool is dispatched")
        .expect("an active waiver can be revoked");
        let revoked = body(&result);
        assert_eq!(revoked["waiver_id"], waiver_id);
        assert_eq!(revoked["status"], "revoked");
        assert_eq!(revoked["revoked_by"], "reviewer");
        assert!(revoked["revoked_at"].is_i64());

        let active_result = dispatch(&engine, "active_waivers", &json!({}))
            .unwrap()
            .unwrap();
        assert_eq!(body(&active_result)["waivers"].as_array().unwrap().len(), 0);

        // clause_waivers is the audit view: a revoked waiver still appears.
        let history_result = dispatch(
            &engine,
            "clause_waivers",
            &json!({ "clause_id": clause_id }),
        )
        .unwrap()
        .unwrap();
        let history = body(&history_result)["waivers"].as_array().unwrap().clone();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["id"], waiver_id);
        assert_eq!(history[0]["status"], "revoked");
    }

    #[test]
    fn revoke_waiver_dispatch_refuses_a_second_revocation() {
        let engine = engine();
        let clause_id = governed_with_waivable_clause(&engine);
        let granted = body(
            &dispatch(
                &engine,
                "grant_waiver",
                &json!({
                    "clause_id": clause_id,
                    "scope": "artifact:crates/**",
                    "reason": "Needed for a deadline.",
                    "expires_at": 4_000_000_000i64,
                    "agent": "monk-eee",
                }),
            )
            .unwrap()
            .unwrap(),
        );
        let waiver_id = granted["waiver_id"].as_str().unwrap().to_string();
        dispatch(
            &engine,
            "revoke_waiver",
            &json!({ "waiver_id": waiver_id, "agent": "reviewer", "reason": "No longer needed." }),
        )
        .unwrap()
        .unwrap();

        let err = dispatch(
            &engine,
            "revoke_waiver",
            &json!({ "waiver_id": waiver_id, "agent": "reviewer", "reason": "Again." }),
        )
        .expect("tool is dispatched")
        .expect_err("a revoked waiver cannot be revoked twice");
        assert!(err.contains("already revoked"), "{err}");
    }

    #[test]
    fn active_waivers_dispatch_reports_nothing_on_a_fresh_engine() {
        let result = dispatch(&engine(), "active_waivers", &json!({}))
            .expect("tool is dispatched")
            .expect("an empty live set is not an error");
        assert_eq!(body(&result)["waivers"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn each_dispatch_function_reports_its_own_missing_required_argument() {
        let engine = engine();
        assert_eq!(
            dispatch(&engine, "grant_waiver", &json!({}))
                .unwrap()
                .unwrap_err(),
            "missing required string arg: clause_id"
        );
        assert_eq!(
            dispatch(&engine, "revoke_waiver", &json!({}))
                .unwrap()
                .unwrap_err(),
            "missing required string arg: waiver_id"
        );
        assert_eq!(
            dispatch(&engine, "clause_waivers", &json!({}))
                .unwrap()
                .unwrap_err(),
            "missing required string arg: clause_id"
        );
    }
}
