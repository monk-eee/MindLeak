//! Minimal MCP stdio transport: newline-delimited JSON-RPC 2.0. Mirrors the
//! MindLeak and Lodestar servers so all three planes speak the identical
//! protocol (ADR-0059).

use std::io::{self, BufRead, Write};

use mindleak_session::SessionRegistry;
use serde_json::{json, Value};

use crate::tools;

const PROTOCOL_VERSION: &str = "2024-11-05";

/// Run the blocking request/response loop until stdin closes.
///
/// `refusal` carries the reason this process may not reach its arbiter. It
/// answers the protocol and refuses every tool call rather than exiting, for
/// the reason `lodestar-mcp` already learned: a process that dies before it can
/// serve anything shows the agent nothing but a server that failed to start,
/// while a refusal returned per call lands where the agent actually reads.
pub fn run<F>(
    endpoint: Option<String>,
    refusal: Option<String>,
    sessions: SessionRegistry,
    environment: F,
) -> io::Result<()>
where
    F: Fn(&str) -> Option<String>,
{
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut line = String::new();
    loop {
        if read_line_lossy(&mut reader, &mut line)? == 0 {
            break; // EOF
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(error) => {
                write_message(
                    &mut out,
                    &error_response(Value::Null, -32700, &format!("parse error: {error}")),
                )?;
                continue;
            }
        };
        if let Some(response) = handle(
            endpoint.as_deref(),
            refusal.as_deref(),
            &sessions,
            &environment,
            &request,
        ) {
            write_message(&mut out, &response)?;
        }
    }
    Ok(())
}

fn handle<F>(
    endpoint: Option<&str>,
    refusal: Option<&str>,
    sessions: &SessionRegistry,
    environment: &F,
    req: &Value,
) -> Option<Value>
where
    F: Fn(&str) -> Option<String>,
{
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => Some(result_response(id?, initialize_result(refusal))),
        "notifications/initialized" | "notifications/cancelled" => None,
        "ping" => Some(result_response(id?, json!({}))),
        "tools/list" => Some(result_response(
            id?,
            json!({ "tools": tools::advertised() }),
        )),
        "tools/call" => {
            let id = id?;
            // Returned per call, not once at startup: a client shows the agent
            // tool results and shows it nothing else. `open_session` is
            // refused here too, exactly like every other tool -- a front door
            // that cannot reach its arbiter has nothing to attribute a session
            // to either.
            if let Some(reason) = refusal {
                return Some(result_response(id, tool_error(reason)));
            }
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            // Intercepted ahead of `tools::call`, exactly how `mindleak-mcp`/
            // `lodestar-mcp` already special-case `open_session`: it needs the
            // session registry, not an Ackplane endpoint (ADR-0137 clause 2).
            let response = if name == tools::OPEN_SESSION {
                let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
                match tools::open_session(sessions, &arguments) {
                    Ok(content) => json!({
                        "content": [{ "type": "text", "text": content.to_string() }],
                        "isError": false
                    }),
                    Err(message) => tool_error(&message),
                }
            } else {
                let endpoint = endpoint.unwrap_or_default();
                match tools::call(endpoint, name, environment) {
                    Ok(content) => json!({
                        "content": [{ "type": "text", "text": content.to_string() }],
                        "isError": false
                    }),
                    Err(message) => tool_error(&message),
                }
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

/// A refused front door still answers `initialize`, and says why there, because
/// a client that never surfaces stderr has no other place to read it.
fn initialize_result(refusal: Option<&str>) -> Value {
    let mut result = json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "ackplane-mcp", "version": env!("CARGO_PKG_VERSION") }
    });
    if let Some(reason) = refusal {
        result["instructions"] = Value::String(reason.to_string());
    }
    result
}

fn tool_error(message: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": message }], "isError": true })
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

/// Replace invalid UTF-8 rather than aborting: `read_line` returns
/// `InvalidData` on non-UTF-8 and would kill the server, so malformed encoding
/// becomes a recoverable parse error like malformed JSON.
fn read_line_lossy(reader: &mut impl BufRead, line: &mut String) -> io::Result<usize> {
    let mut bytes = Vec::new();
    let read = reader.read_until(b'\n', &mut bytes)?;
    line.clear();
    line.push_str(&String::from_utf8_lossy(&bytes));
    Ok(read)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn sessions() -> SessionRegistry {
        SessionRegistry::new("test").unwrap()
    }

    fn request(id: i64, method: &str, params: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
    }

    #[test]
    fn initialize_reports_this_server_and_its_version() {
        let response = handle(
            Some("http://127.0.0.1:8443"),
            None,
            &sessions(),
            &no_env,
            &request(1, "initialize", json!({})),
        )
        .expect("initialize is answered");
        assert_eq!(response["result"]["serverInfo"]["name"], "ackplane-mcp");
        assert_eq!(response["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert!(response["result"]["instructions"].is_null());
    }

    #[test]
    fn tools_list_advertises_the_translated_tools() {
        let response = handle(
            Some("http://127.0.0.1:8443"),
            None,
            &sessions(),
            &no_env,
            &request(2, "tools/list", json!({})),
        )
        .expect("tools/list is answered");
        let tools = response["result"]["tools"].as_array().expect("an array");
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0]["name"], tools::OPEN_SESSION);
        assert_eq!(tools[1]["name"], tools::CHECK_ENROLLMENT_STATUS);
        assert_eq!(tools[2]["name"], tools::ACTIVE_CLAIMS);
    }

    /// The clause-4 refusal has to reach the agent, and a tool result is the
    /// only channel a client reliably surfaces.
    #[test]
    fn a_refused_front_door_answers_the_protocol_and_refuses_every_call() {
        let refusal = "refused: not a loopback endpoint";
        let initialize = handle(
            None,
            Some(refusal),
            &sessions(),
            &no_env,
            &request(1, "initialize", json!({})),
        )
        .expect("a refused server still completes the handshake");
        assert_eq!(initialize["result"]["instructions"], refusal);

        let called = handle(
            None,
            Some(refusal),
            &sessions(),
            &no_env,
            &request(
                3,
                "tools/call",
                json!({ "name": tools::CHECK_ENROLLMENT_STATUS, "arguments": {} }),
            ),
        )
        .expect("a refused server answers tools/call");
        assert_eq!(called["result"]["isError"], true);
        assert_eq!(called["result"]["content"][0]["text"], refusal);
    }

    /// `open_session` is refused exactly like every other tool when this front
    /// door cannot reach its arbiter -- there is no weaker, arbiter-free path
    /// through the refusal (ADR-0137's identity layers on top of connection
    /// trust; it never substitutes for it).
    #[test]
    fn a_refused_front_door_refuses_open_session_too() {
        let refusal = "refused: not a loopback endpoint";
        let called = handle(
            None,
            Some(refusal),
            &sessions(),
            &no_env,
            &request(
                3,
                "tools/call",
                json!({ "name": tools::OPEN_SESSION, "arguments": { "session_id": "0123456789abcdef0123456789abcdef" } }),
            ),
        )
        .expect("a refused server answers tools/call");
        assert_eq!(called["result"]["isError"], true);
        assert_eq!(called["result"]["content"][0]["text"], refusal);
    }

    /// The whole point of ADR-0137 clause 2: a real session opens without ever
    /// touching the (here, absent) Ackplane endpoint.
    #[test]
    fn open_session_succeeds_with_no_endpoint_reachable() {
        let response = handle(
            None,
            None,
            &sessions(),
            &no_env,
            &request(
                3,
                "tools/call",
                json!({ "name": tools::OPEN_SESSION, "arguments": { "session_id": "0123456789abcdef0123456789abcdef" } }),
            ),
        )
        .expect("open_session is answered");
        assert_eq!(response["result"]["isError"], false);
        let content: Value = response["result"]["content"][0]["text"]
            .as_str()
            .and_then(|text| serde_json::from_str(text).ok())
            .expect("a JSON body");
        assert!(content["agent_id"]
            .as_str()
            .expect("agent_id is a string")
            .starts_with("session:v1:"));
    }

    #[test]
    fn a_notification_earns_no_response() {
        assert!(handle(
            Some("http://127.0.0.1:8443"),
            None,
            &sessions(),
            &no_env,
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        )
        .is_none());
    }

    #[test]
    fn an_unknown_method_is_a_protocol_error_naming_it() {
        let response = handle(
            Some("http://127.0.0.1:8443"),
            None,
            &sessions(),
            &no_env,
            &request(4, "resources/list", json!({})),
        )
        .expect("an identified request earns an answer");
        assert_eq!(response["error"]["code"], -32601);
        assert!(response["error"]["message"]
            .as_str()
            .expect("a message")
            .contains("resources/list"));
    }

    #[test]
    fn malformed_utf8_is_recoverable_rather_than_fatal() {
        let mut input: &[u8] = b"\xff\xfe\n{}\n";
        let mut line = String::new();
        assert!(read_line_lossy(&mut input, &mut line).expect("invalid utf8 must not error") > 0);
        assert!(read_line_lossy(&mut input, &mut line).expect("valid line") > 0);
        assert_eq!(line.trim(), "{}");
    }
}
