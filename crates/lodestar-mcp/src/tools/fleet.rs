use lodestar_core::fleet::{FleetView, Staleness};
use lodestar_core::Lodestar;
use serde_json::{json, Value};

use super::rendered;

pub(super) fn definitions() -> Vec<Value> {
    vec![json!({
        "name": "fleet_view",
        "description": "Read-only: who is working where, derived from the context sessions declared on open_session (ADR-0035). Reports each live session's branch/head/base, how far behind its base it said it was, and whether live sessions disagree about their base. Advisory only — every value is self-reported, so under the ADR-0034 ceiling rule this caps at review and can never block. Undeclared values report unknown rather than being guessed.",
        "inputSchema": { "type": "object", "properties": {} }
    })]
}

pub(super) fn dispatch(
    engine: &Lodestar,
    name: &str,
    _args: &Value,
) -> Option<Result<Value, String>> {
    match name {
        "fleet_view" => Some((|| {
            let view = engine.fleet_view().map_err(|e| e.to_string())?;
            rendered(render(&view), &view)
        })()),
        _ => None,
    }
}

/// Small enough to read inline in a chat pane, so it renders as Markdown while
/// `structuredContent` carries the machine-readable form.
fn render(view: &FleetView) -> String {
    let mut out = String::from("## Fleet\n\n");
    if view.sessions.is_empty() {
        out.push_str("No session has declared where it is working.\n\n");
    } else {
        out.push_str("| Agent | Branch | Base | Behind | Claims |\n|---|---|---|---|---|\n");
        for session in &view.sessions {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                session.agent_id,
                session.context.branch.as_deref().unwrap_or("unknown"),
                session.context.base.as_deref().unwrap_or("unknown"),
                staleness_label(session.staleness),
                session.claimed_task_ids.len()
            ));
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "Bases in use: {}. Sessions with claims but no declared base: {}. Diverged: {}.\n\n{}\n",
        if view.divergence.bases.is_empty() {
            "none declared".to_string()
        } else {
            view.divergence.bases.join(", ")
        },
        view.divergence.undeclared_sessions,
        if view.divergence.diverged {
            "yes"
        } else {
            "no"
        },
        view.enforcement
    ));
    out
}

fn staleness_label(staleness: Staleness) -> String {
    match staleness {
        Staleness::Unknown => "unknown".to_string(),
        Staleness::Current => "current".to_string(),
        Staleness::Behind(count) => format!("{count}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lodestar_core::model::GoalKind;

    fn engine() -> Lodestar {
        Lodestar::open_in_memory().unwrap()
    }

    // ADR-0035 decisions 4 and 5: the view reports what was declared, says
    // `unknown` where nothing was, and states its own ceiling so no caller can
    // mistake an advisory signal for a gate.
    #[test]
    fn fleet_view_reports_declared_context_and_never_claims_authority() {
        let engine = engine();
        let goal = engine
            .define_goal(GoalKind::Objective, "Fleet", "coordinate", None)
            .unwrap();
        let task = engine.create_task(&goal.id, "Work", "done").unwrap();
        assert!(engine.claim_task(&task.id, "agent-a", 600).unwrap());
        engine
            .declare_session_context(
                "agent-a",
                &mindleak_session::SessionContext {
                    branch: Some("fleet/work".into()),
                    head_sha: Some("abc1234".into()),
                    base: Some("origin/main".into()),
                    dirty: Some(false),
                    behind: Some(4),
                },
            )
            .unwrap();

        let result = dispatch(&engine, "fleet_view", &json!({}))
            .expect("tool is dispatched")
            .expect("tool succeeds");
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("origin/main"), "{text}");
        assert!(text.contains("advisory"), "{text}");
        let structured = &result["structuredContent"];
        assert_eq!(structured["sessions"][0]["agent_id"], "agent-a");
        assert_eq!(structured["sessions"][0]["staleness"]["commits"], 4);
        assert_eq!(structured["divergence"]["diverged"], false);
    }

    #[test]
    fn an_empty_fleet_says_so_rather_than_rendering_a_blank_table() {
        let engine = engine();
        let result = dispatch(&engine, "fleet_view", &json!({}))
            .expect("tool is dispatched")
            .expect("tool succeeds");
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("No session has declared"), "{text}");
    }

    #[test]
    fn unknown_tools_are_not_claimed_by_this_module() {
        assert!(dispatch(&engine(), "board", &json!({})).is_none());
    }
}
