//! Minimal MCP stdio transport: newline-delimited JSON-RPC 2.0.

use std::io::{self, BufRead, Write};

use mindleak_core::MindLeak;
use mindleak_session::SessionRegistry;
use mindleak_storage::{RunningBinary, StorageStatus};
use serde_json::{json, Value};

use crate::maintenance::ActivitySignal;
use crate::tools;

const PROTOCOL_VERSION: &str = "2024-11-05";

/// Run the blocking request/response loop until stdin closes.
///
/// `stale_build` carries the notice when this binary is behind the checkout it
/// serves, so `open_session` can tell the agent rather than only the log.
/// `coordination_refusal` is the stronger case: this process never settled
/// which arbiter owns the repository, so it answers the protocol but performs
/// no work at all.
/// `running` answers the different question of whether the file this process
/// started from has since been replaced, which can only be asked per call.
pub fn run(
    engine: MindLeak,
    sessions: SessionRegistry,
    activity: ActivitySignal,
    storage: StorageStatus,
    stale_build: Option<String>,
    coordination_refusal: Option<String>,
    running: RunningBinary,
) -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut line = String::new();
    loop {
        let read = read_line_lossy(&mut reader, &mut line)?;
        if read == 0 {
            break; // EOF
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let _request_activity = activity.begin_request();

        let request: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                write_message(
                    &mut out,
                    &error_response(Value::Null, -32700, &format!("parse error: {e}")),
                )?;
                continue;
            }
        };

        if let Some(response) = handle(
            &engine,
            &sessions,
            &storage,
            stale_build.as_deref(),
            coordination_refusal.as_deref(),
            &running,
            &request,
        ) {
            write_message(&mut out, &response)?;
        }
    }
    Ok(())
}

fn handle(
    engine: &MindLeak,
    sessions: &SessionRegistry,
    storage: &StorageStatus,
    stale_build: Option<&str>,
    coordination_refusal: Option<&str>,
    running: &RunningBinary,
    req: &Value,
) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => Some(result_response(id?, initialize_result())),
        "notifications/initialized" | "notifications/cancelled" => None,
        "ping" => Some(result_response(id?, json!({}))),
        "tools/list" => Some(result_response(id?, json!({ "tools": tools::list() }))),
        "tools/call" => {
            let id = id?;
            // Answering would be the second arbiter ADR-0082 forbids, so the
            // refusal is the only work this process does. It is returned per
            // call rather than once at startup because a client shows the
            // agent tool results, and shows it nothing else.
            if let Some(notice) = coordination_refusal {
                return Some(result_response(id, tool_error(notice)));
            }
            let response = match tools::bind_session(&params, sessions).and_then(|bound| {
                tools::call_with_storage(engine, &bound, Some(storage), stale_build, running)
            }) {
                Ok(content) => content,
                Err(msg) => tool_error(&msg),
            };
            Some(result_response(id, response))
        }
        "shutdown" => Some(result_response(id?, Value::Null)),
        other => match id {
            Some(id) if !id.is_null() => Some(error_response(
                id,
                -32601,
                &format!("method not found: {other}"),
            )),
            _ => None,
        },
    }
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": {
            "name": "mindleak-mcp",
            "version": format!(
                "{}+{}",
                env!("CARGO_PKG_VERSION"),
                env!("MINDLEAK_BUILD_SHA")
            )
        }
    })
}

fn tool_error(message: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true
    })
}

fn result_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn write_message(out: &mut impl Write, value: &Value) -> io::Result<()> {
    let text = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    out.write_all(text.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()
}

/// Read one newline-terminated line, replacing invalid UTF-8 rather than
/// aborting: `read_line` returns `InvalidData` on non-UTF-8 and would kill the
/// server, so malformed encoding becomes a recoverable parse error like
/// malformed JSON. Only a genuine I/O error still propagates.
fn read_line_lossy(reader: &mut impl BufRead, line: &mut String) -> io::Result<usize> {
    let mut bytes = Vec::new();
    let read = reader.read_until(b'\n', &mut bytes)?;
    line.clear();
    line.push_str(&String::from_utf8_lossy(&bytes));
    Ok(read)
}

#[cfg(test)]
mod tests {
    // Generated by AI (UnitTest MCP)
    use super::*;
    use mindleak_core::MindLeak;
    use mindleak_storage::{DatabaseOrigin, StorageStatus};
    use std::path::PathBuf;

    #[test]
    fn read_line_lossy_recovers_invalid_utf8_instead_of_failing() {
        use std::io::Cursor;
        // A lone 0xFF/0xFE is not valid UTF-8; read_line would return InvalidData.
        let mut input = Cursor::new(b"\xff\xfe oops\n{\"ok\":1}\n".to_vec());
        let mut line = String::new();

        let first = read_line_lossy(&mut input, &mut line).expect("invalid utf8 must not error");
        assert!(first > 0);
        assert!(
            line.contains('\u{FFFD}'),
            "invalid bytes should become the replacement char, got {line:?}"
        );

        let second = read_line_lossy(&mut input, &mut line).expect("valid line");
        assert!(second > 0);
        assert_eq!(line.trim(), "{\"ok\":1}");

        assert_eq!(read_line_lossy(&mut input, &mut line).unwrap(), 0);
    }

    fn engine() -> MindLeak {
        MindLeak::open_in_memory().unwrap()
    }

    /// These tests are about routing, not about build identity or coordination,
    /// so they run as a current build that settled its arbiter. Shadowing here
    /// keeps both arguments out of every call site that does not care about
    /// them.
    fn handle(
        engine: &MindLeak,
        sessions: &SessionRegistry,
        storage: &StorageStatus,
        req: &Value,
    ) -> Option<Value> {
        super::handle(
            engine,
            sessions,
            storage,
            None,
            None,
            &RunningBinary::from_parts(None, None),
            req,
        )
    }

    fn sessions() -> SessionRegistry {
        SessionRegistry::new("test").unwrap()
    }

    fn storage() -> StorageStatus {
        StorageStatus {
            plane: "mindleak".into(),
            repository_id: Some("0123456789abcdef0123456789abcdef".into()),
            database_path: PathBuf::from("state/graph.db"),
            origin: DatabaseOrigin::Repository,
            legacy_path: None,
            migrated_legacy: false,
        }
    }

    /// A real refusal, not a placeholder: the text is what the agent has to be
    /// able to act on.
    fn refusal() -> String {
        ackplane_core::CoordinationModeError::NoFederationClient.refusal_notice()
    }

    fn open_session_request() -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": {
                "name": "open_session",
                "arguments": { "session_id": "0123456789abcdef0123456789abcdef" }
            }
        })
    }

    #[test]
    fn a_refused_coordination_mode_stops_a_call_that_would_otherwise_have_worked() {
        let engine = engine();
        let sessions = sessions();
        let storage = storage();
        let running = RunningBinary::from_parts(None, None);
        let request = open_session_request();

        // Establish first that this call really does coordinate when the mode
        // resolved. Without this the refusal test could pass against a call
        // that was going to fail for its own reasons.
        let served =
            super::handle(&engine, &sessions, &storage, None, None, &running, &request).unwrap();
        assert_ne!(
            served["result"]["isError"],
            json!(true),
            "open_session must succeed when no refusal is carried: {served}"
        );

        let notice = refusal();
        let refused = super::handle(
            &engine,
            &sessions,
            &storage,
            None,
            Some(&notice),
            &running,
            &request,
        )
        .unwrap();

        assert_eq!(
            refused["result"],
            json!({
                "content": [{ "type": "text", "text": notice }],
                "isError": true
            }),
            "a refusing server must answer the reason and do nothing else"
        );
    }

    #[test]
    fn a_refused_coordination_mode_still_completes_the_handshake() {
        // "The server failed to start" is the message that sent an operator
        // hunting a broken binary instead of a wrong declaration, so the
        // handshake has to survive for the refusal to be reachable at all.
        let engine = engine();
        let sessions = sessions();
        let storage = storage();
        let running = RunningBinary::from_parts(None, None);
        let notice = refusal();

        for method in ["initialize", "tools/list"] {
            let request = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": {} });
            let response = super::handle(
                &engine,
                &sessions,
                &storage,
                None,
                Some(&notice),
                &running,
                &request,
            )
            .unwrap_or_else(|| panic!("{method} must still answer while refusing"));

            assert!(
                response["error"].is_null(),
                "{method} must not error while refusing: {response}"
            );
        }
    }

    #[test]
    fn initialize_reports_server_info() {
        let req = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} });
        let resp = handle(&engine(), &sessions(), &storage(), &req).unwrap();
        assert_eq!(resp["result"]["serverInfo"]["name"], "mindleak-mcp");
        assert_eq!(
            resp["result"]["serverInfo"]["version"],
            format!(
                "{}+{}",
                env!("CARGO_PKG_VERSION"),
                env!("MINDLEAK_BUILD_SHA")
            )
        );
        assert_eq!(resp["result"]["protocolVersion"], PROTOCOL_VERSION);
    }

    #[test]
    fn tools_list_returns_tool_array() {
        let req = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} });
        let resp = handle(&engine(), &sessions(), &storage(), &req).unwrap();
        assert!(resp["result"]["tools"].as_array().unwrap().len() >= 10);
    }

    #[test]
    fn tools_call_returns_content() {
        let req = json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "graph_stats", "arguments": {} }
        });
        let resp = handle(&engine(), &sessions(), &storage(), &req).unwrap();
        assert!(resp["result"]["content"][0]["text"].is_string());
    }

    #[test]
    fn notification_produces_no_response() {
        let req = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle(&engine(), &sessions(), &storage(), &req).is_none());
    }

    #[test]
    fn ping_returns_empty_result() {
        let req = json!({ "jsonrpc": "2.0", "id": 4, "method": "ping" });
        let resp = handle(&engine(), &sessions(), &storage(), &req).unwrap();
        assert_eq!(resp["result"], json!({}));
    }

    #[test]
    fn unknown_method_returns_method_not_found() {
        let req = json!({ "jsonrpc": "2.0", "id": 5, "method": "does_not_exist" });
        let resp = handle(&engine(), &sessions(), &storage(), &req).unwrap();
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[test]
    fn multiplexed_sessions_produce_distinct_evidence() {
        let engine = engine();
        let sessions = sessions();
        let storage = storage();
        for (id, token, sha, path) in [
            (
                10,
                "00112233445566778899aabbccddeeff",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "src/a.rs",
            ),
            (
                11,
                "ffeeddccbbaa99887766554433221100",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "src/b.rs",
            ),
        ] {
            let open = json!({
                "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": { "name": "open_session", "arguments": { "session_id": token } }
            });
            handle(&engine, &sessions, &storage, &open).unwrap();
            let ingest = json!({
                "jsonrpc": "2.0", "id": id + 10, "method": "tools/call",
                "params": { "name": "ingest_commit", "arguments": {
                    "session_id": token,
                    "agent": "impersonator",
                    "message": sha,
                    "sha": sha,
                    "changed_files": [path],
                    "timestamp": 100
                }}
            });
            handle(&engine, &sessions, &storage, &ingest).unwrap();
        }

        let evidence = |id, token: &str| {
            let request = json!({
                "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": { "name": "evidence_for", "arguments": {
                    "session_id": token, "started_at": 90, "ended_at": 110
                }}
            });
            let response = handle(&engine, &sessions, &storage, &request).unwrap();
            serde_json::from_str::<Value>(
                response["result"]["content"][0]["text"].as_str().unwrap(),
            )
            .unwrap()
        };
        let first = evidence(30, "00112233445566778899aabbccddeeff");
        let second = evidence(31, "ffeeddccbbaa99887766554433221100");
        assert_ne!(first["agent_id"], second["agent_id"]);
        assert_ne!(first["agent_id"], "impersonator");
        // ADR-0054: `session:v1:{fingerprint}` — no label segment, so exactly
        // two colons whatever this process was named.
        let first_id = first["agent_id"].as_str().unwrap();
        assert!(first_id.starts_with("session:v1:"));
        assert_eq!(first_id.matches(':').count(), 2);
        assert_eq!(
            first["commit_ids"],
            json!(["intent:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"])
        );
        assert_eq!(
            second["commit_ids"],
            json!(["intent:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"])
        );
        assert_eq!(first["changed_node_ids"], json!(["artifact:src/a.rs"]));
        assert_eq!(second["changed_node_ids"], json!(["artifact:src/b.rs"]));
    }
}
