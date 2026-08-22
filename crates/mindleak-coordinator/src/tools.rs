//! The coordinator's composed tools (ADR-0097 decisions 2-3): one
//! `open_session` that opens both planes and verifies they agree on identity,
//! and one `preflight` that runs the reads a write should already have made
//! and reports them with per-plane provenance, never silently downgrading a
//! failed plane into an empty result that looks like agreement.

use std::io::{BufRead, Write};

use serde_json::{json, Value};

use crate::child::ChildClient;

/// ADR-0097 decision 2: one call opens both planes, forwards the same
/// declared context to each, and reports whether they resolved the same
/// agent and repository identity. A partial open names the failed plane
/// rather than presenting one plane's state as if it were both.
pub fn open_session<R1, W1, R2, W2>(
    mindleak: &mut ChildClient<R1, W1>,
    lodestar: &mut ChildClient<R2, W2>,
    arguments: Value,
) -> Value
where
    R1: BufRead,
    W1: Write,
    R2: BufRead,
    W2: Write,
{
    let mindleak_reply = mindleak.call_tool("open_session", arguments.clone());
    let mindleak_status = mindleak.call_tool("storage_status", json!({}));
    let lodestar_reply = lodestar.call_tool("open_session", arguments.clone());
    let lodestar_status = lodestar.call_tool("storage_status", json!({}));

    let identity = compare_identity(
        &mindleak_reply,
        &mindleak_status,
        &lodestar_reply,
        &lodestar_status,
    );

    json!({
        "mindleak": plane_report(&mindleak_reply, &mindleak_status),
        "lodestar": plane_report(&lodestar_reply, &lodestar_status),
        "identity": identity,
        "both_open": mindleak_reply.is_ok() && lodestar_reply.is_ok(),
    })
}

/// ADR-0097 decision 3: given paths and symbols, compose the reads a write
/// should already have made — MindLeak's structural/footprint overlap,
/// Lodestar's live-claim overlap, and Lodestar's governing-clause advice —
/// into one result. Each read stays independently attributable so a client
/// can show "both agree it's clear" as a materially different answer from
/// "one plane never answered."
pub fn preflight<R1, W1, R2, W2>(
    mindleak: &mut ChildClient<R1, W1>,
    lodestar: &mut ChildClient<R2, W2>,
    session_id: &str,
    paths: &[String],
    symbols: &[String],
    task_id: Option<&str>,
) -> Value
where
    R1: BufRead,
    W1: Write,
    R2: BufRead,
    W2: Write,
{
    let footprint = mindleak.call_tool(
        "check_overlap",
        json!({ "session_id": session_id, "paths": paths, "symbols": symbols }),
    );
    let claims = lodestar.call_tool(
        "task_query",
        json!({ "view": "overlap", "session_id": session_id, "paths": paths, "symbols": symbols }),
    );
    let mut node_ids: Vec<String> = paths
        .iter()
        .map(|path| format!("artifact:{path}"))
        .collect();
    node_ids.extend(symbols.iter().cloned());
    let mut advise_args = json!({ "node_ids": node_ids });
    if let Some(task_id) = task_id {
        advise_args["task_id"] = json!(task_id);
    }
    let advice = lodestar.call_tool("advise", advise_args);

    json!({
        "footprint": result_report(&footprint),
        "claims": result_report(&claims),
        "advice": result_report(&advice),
        "all_answered": footprint.is_ok() && claims.is_ok() && advice.is_ok(),
    })
}

fn plane_report(reply: &Result<Value, String>, status: &Result<Value, String>) -> Value {
    match reply {
        Ok(reply) => {
            let mut report = json!({ "status": "ok", "reply": reply });
            if let Ok(status) = status {
                report["repository_id"] =
                    status.get("repository_id").cloned().unwrap_or(Value::Null);
                report["version"] = status.get("version").cloned().unwrap_or(Value::Null);
            }
            report
        }
        Err(error) => json!({ "status": "failed", "error": error }),
    }
}

fn result_report(result: &Result<Value, String>) -> Value {
    match result {
        Ok(value) => json!({ "status": "ok", "result": value }),
        Err(error) => json!({ "status": "failed", "error": error }),
    }
}

/// `None` when either plane's `open_session` itself failed — a plane that
/// never answered has no identity to compare, and reporting `false` there
/// would read as "they disagreed" rather than "one of them is unreachable".
fn compare_identity(
    mindleak_reply: &Result<Value, String>,
    mindleak_status: &Result<Value, String>,
    lodestar_reply: &Result<Value, String>,
    lodestar_status: &Result<Value, String>,
) -> Value {
    let agents_match = match (
        mindleak_reply
            .as_ref()
            .ok()
            .and_then(|v| v.get("agent_id"))
            .and_then(Value::as_str),
        lodestar_reply
            .as_ref()
            .ok()
            .and_then(|v| v.get("agent_id"))
            .and_then(Value::as_str),
    ) {
        (Some(a), Some(b)) => Some(a == b),
        _ => None,
    };
    let repositories_match = match (
        mindleak_status
            .as_ref()
            .ok()
            .and_then(|v| v.get("repository_id"))
            .and_then(Value::as_str),
        lodestar_status
            .as_ref()
            .ok()
            .and_then(|v| v.get("repository_id"))
            .and_then(Value::as_str),
    ) {
        (Some(a), Some(b)) => Some(a == b),
        _ => None,
    };
    json!({ "agents_match": agents_match, "repositories_match": repositories_match })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn client_with_canned_responses(responses: &[&str]) -> ChildClient<Cursor<Vec<u8>>, Vec<u8>> {
        let mut bytes = Vec::new();
        for response in responses {
            bytes.extend_from_slice(response.as_bytes());
            bytes.push(b'\n');
        }
        ChildClient::new(Cursor::new(bytes), Vec::new())
    }

    fn tool_reply(structured: Value) -> String {
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "content": [{"type": "text", "text": "ignored"}], "structuredContent": structured }
        })
        .to_string()
    }

    #[test]
    fn open_session_reports_both_planes_and_matching_identity() {
        let mindleak_open = tool_reply(json!({ "agent_id": "session:v1:abc" }));
        let mindleak_status = tool_reply(json!({ "repository_id": "repo-1", "version": "0.1.7" }));
        let lodestar_open = tool_reply(json!({ "agent_id": "session:v1:abc", "context": {} }));
        let lodestar_status = tool_reply(json!({ "repository_id": "repo-1", "version": "0.1.7" }));

        let mut mindleak = client_with_canned_responses(&[&mindleak_open, &mindleak_status]);
        let mut lodestar = client_with_canned_responses(&[&lodestar_open, &lodestar_status]);

        let result = open_session(&mut mindleak, &mut lodestar, json!({ "session_id": "tok" }));

        assert_eq!(result["both_open"], true);
        assert_eq!(result["identity"]["agents_match"], true);
        assert_eq!(result["identity"]["repositories_match"], true);
        assert_eq!(result["mindleak"]["status"], "ok");
        assert_eq!(result["lodestar"]["status"], "ok");
    }

    #[test]
    fn open_session_names_the_failed_plane_rather_than_hiding_it() {
        // MindLeak answers "tools/call" with a JSON-RPC error; Lodestar never
        // gets a second canned reply for its own storage_status call, which
        // fails as EOF — both are real, distinct failure shapes.
        let mindleak_error =
            json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32000, "message": "unavailable" } })
                .to_string();
        let lodestar_open = tool_reply(json!({ "agent_id": "session:v1:abc" }));

        let mut mindleak = client_with_canned_responses(&[&mindleak_error, &mindleak_error]);
        let mut lodestar = client_with_canned_responses(&[&lodestar_open]);

        let result = open_session(&mut mindleak, &mut lodestar, json!({ "session_id": "tok" }));

        assert_eq!(result["both_open"], false);
        assert_eq!(result["mindleak"]["status"], "failed");
        assert!(result["mindleak"]["error"]
            .as_str()
            .unwrap()
            .contains("unavailable"));
        assert_eq!(result["lodestar"]["status"], "ok");
        // Neither agent_id nor repository_id could be compared when a plane
        // never answered, so this must read as "unknown", not "false".
        assert_eq!(result["identity"]["agents_match"], Value::Null);
    }

    #[test]
    fn open_session_flags_a_real_identity_mismatch() {
        let mindleak_open = tool_reply(json!({ "agent_id": "session:v1:abc" }));
        let mindleak_status = tool_reply(json!({ "repository_id": "repo-1" }));
        let lodestar_open = tool_reply(json!({ "agent_id": "session:v1:different" }));
        let lodestar_status = tool_reply(json!({ "repository_id": "repo-1" }));

        let mut mindleak = client_with_canned_responses(&[&mindleak_open, &mindleak_status]);
        let mut lodestar = client_with_canned_responses(&[&lodestar_open, &lodestar_status]);

        let result = open_session(&mut mindleak, &mut lodestar, json!({ "session_id": "tok" }));

        assert_eq!(result["identity"]["agents_match"], false);
        assert_eq!(result["identity"]["repositories_match"], true);
    }

    #[test]
    fn preflight_composes_all_three_reads() {
        let footprint = tool_reply(json!({ "recent_footprints": [] }));
        let overlap = tool_reply(json!({ "same_branch_collision": [] }));
        let advice = tool_reply(json!({ "disposition": "advise", "governing": [] }));

        let mut mindleak = client_with_canned_responses(&[&footprint]);
        let mut lodestar = client_with_canned_responses(&[&overlap, &advice]);

        let result = preflight(
            &mut mindleak,
            &mut lodestar,
            "tok",
            &["crates/a.rs".to_string()],
            &[],
            None,
        );

        assert_eq!(result["all_answered"], true);
        assert_eq!(result["footprint"]["status"], "ok");
        assert_eq!(result["claims"]["status"], "ok");
        assert_eq!(result["advice"]["status"], "ok");
        assert_eq!(result["advice"]["result"]["disposition"], "advise");
    }

    #[test]
    fn preflight_converts_paths_into_artifact_node_ids_for_advise() {
        let footprint = tool_reply(json!({}));
        let overlap = tool_reply(json!({}));
        let advice = tool_reply(json!({ "disposition": "advise" }));

        let mut mindleak = client_with_canned_responses(&[&footprint]);
        let mut lodestar = client_with_canned_responses(&[&overlap, &advice]);

        preflight(
            &mut mindleak,
            &mut lodestar,
            "tok",
            &["crates/a.rs".to_string()],
            &["symbol:crates/a.rs:Foo".to_string()],
            Some("task:123"),
        );

        let written = String::from_utf8(lodestar.into_writer()).expect("utf8 requests");
        let advise_request = written
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("valid json"))
            .find(|request| request["params"]["name"] == "advise")
            .expect("an advise call was made");
        let node_ids = advise_request["params"]["arguments"]["node_ids"]
            .as_array()
            .unwrap();
        assert_eq!(node_ids[0], "artifact:crates/a.rs");
        assert_eq!(node_ids[1], "symbol:crates/a.rs:Foo");
        assert_eq!(advise_request["params"]["arguments"]["task_id"], "task:123");
    }

    #[test]
    fn preflight_reports_a_failed_read_without_failing_the_others() {
        let footprint_error =
            json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32000, "message": "down" } })
                .to_string();
        let overlap = tool_reply(json!({}));
        let advice = tool_reply(json!({ "disposition": "advise" }));

        let mut mindleak = client_with_canned_responses(&[&footprint_error]);
        let mut lodestar = client_with_canned_responses(&[&overlap, &advice]);

        let result = preflight(&mut mindleak, &mut lodestar, "tok", &[], &[], None);

        assert_eq!(result["all_answered"], false);
        assert_eq!(result["footprint"]["status"], "failed");
        assert_eq!(result["claims"]["status"], "ok");
        assert_eq!(result["advice"]["status"], "ok");
    }
}
