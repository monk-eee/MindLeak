//! The coordinator's own stdio MCP surface (ADR-0097 decision 1): a thin
//! front exposing two composed tools over the two spawned children. It holds
//! no graph or intent store of its own — every tool call proxies straight
//! through to MindLeak and/or Lodestar. Mirrors `lodestar-mcp`/`mindleak-mcp`'s
//! own transport so all three speak the identical stdio JSON-RPC shape.

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

use crate::child::ChildClient;
use crate::tools;

const PROTOCOL_VERSION: &str = "2024-11-05";

/// Run the blocking request/response loop until stdin closes.
pub fn run<R1, W1, R2, W2>(
    mindleak: &mut ChildClient<R1, W1>,
    lodestar: &mut ChildClient<R2, W2>,
) -> io::Result<()>
where
    R1: BufRead,
    W1: Write,
    R2: BufRead,
    W2: Write,
{
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            break; // EOF
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
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
        if let Some(response) = handle(mindleak, lodestar, &request) {
            write_message(&mut out, &response)?;
        }
    }
    Ok(())
}

fn handle<R1, W1, R2, W2>(
    mindleak: &mut ChildClient<R1, W1>,
    lodestar: &mut ChildClient<R2, W2>,
    req: &Value,
) -> Option<Value>
where
    R1: BufRead,
    W1: Write,
    R2: BufRead,
    W2: Write,
{
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => Some(result_response(id?, initialize_result())),
        "notifications/initialized" | "notifications/cancelled" => None,
        "ping" => Some(result_response(id?, json!({}))),
        "tools/list" => Some(result_response(id?, json!({ "tools": advertised() }))),
        "tools/call" => {
            let id = id?;
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            Some(result_response(
                id,
                dispatch(mindleak, lodestar, name, arguments),
            ))
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

fn dispatch<R1, W1, R2, W2>(
    mindleak: &mut ChildClient<R1, W1>,
    lodestar: &mut ChildClient<R2, W2>,
    name: &str,
    arguments: Value,
) -> Value
where
    R1: BufRead,
    W1: Write,
    R2: BufRead,
    W2: Write,
{
    match name {
        "coordinator_open_session" => {
            let composed = tools::open_session(mindleak, lodestar, arguments);
            rendered(open_session_summary(&composed), composed)
        }
        "coordinator_preflight" => {
            let session_id = arguments
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let paths = string_array(&arguments, "paths");
            let symbols = string_array(&arguments, "symbols");
            let task_id = arguments
                .get("task_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            let composed = tools::preflight(
                mindleak,
                lodestar,
                &session_id,
                &paths,
                &symbols,
                task_id.as_deref(),
            );
            rendered(preflight_summary(&composed), composed)
        }
        other => tool_error(&format!("unknown tool: {other}")),
    }
}

fn string_array(arguments: &Value, key: &str) -> Vec<String> {
    arguments
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn open_session_summary(composed: &Value) -> String {
    if composed["both_open"] == json!(true) {
        format!(
            "Both planes opened. agents_match={}, repositories_match={}",
            composed["identity"]["agents_match"], composed["identity"]["repositories_match"]
        )
    } else {
        format!(
            "Partial open — mindleak: {}, lodestar: {}",
            composed["mindleak"]["status"], composed["lodestar"]["status"]
        )
    }
}

fn preflight_summary(composed: &Value) -> String {
    if composed["all_answered"] == json!(true) {
        "Preflight complete: footprint, claims, and advice all answered.".to_string()
    } else {
        format!(
            "Preflight partial — footprint: {}, claims: {}, advice: {}",
            composed["footprint"]["status"],
            composed["claims"]["status"],
            composed["advice"]["status"]
        )
    }
}

fn advertised() -> Value {
    json!([
        {
            "name": "coordinator_open_session",
            "description": "ADR-0097 decision 2: open both MindLeak and Lodestar in one call, forwarding the same declared context to each and reporting whether they resolved the same agent and repository identity. A partial open names the failed plane rather than presenting one plane's state as if it were both.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "branch": { "type": "string" },
                    "head_sha": { "type": "string" },
                    "base": { "type": "string" },
                    "dirty": { "type": "boolean" },
                    "behind": { "type": "integer", "minimum": 0 }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "coordinator_preflight",
            "description": "ADR-0097 decision 3: given paths and symbols, compose MindLeak's check_overlap, Lodestar's task_query(view=overlap), and Lodestar's advise into one result with per-plane provenance, before a write.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "paths": { "type": "array", "items": { "type": "string" } },
                    "symbols": { "type": "array", "items": { "type": "string" } },
                    "task_id": { "type": "string" }
                },
                "required": ["session_id"]
            }
        }
    ])
}

fn rendered(markdown: impl Into<String>, structured: Value) -> Value {
    json!({
        "content": [{ "type": "text", "text": markdown.into() }],
        "structuredContent": structured,
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

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": {
            "name": "mindleak-coordinator",
            "version": format!("{}+{}", env!("CARGO_PKG_VERSION"), env!("MINDLEAK_BUILD_SHA"))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::child::ChildClient;
    use std::io::Cursor;

    fn client_with_canned_responses(responses: &[Value]) -> ChildClient<Cursor<Vec<u8>>, Vec<u8>> {
        let mut bytes = Vec::new();
        for response in responses {
            bytes.extend_from_slice(response.to_string().as_bytes());
            bytes.push(b'\n');
        }
        ChildClient::new(Cursor::new(bytes), Vec::new())
    }

    fn tool_result(structured: Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "content": [{"type": "text", "text": "x"}], "structuredContent": structured }
        })
    }

    #[test]
    fn initialize_reports_server_identity() {
        let mut mindleak = client_with_canned_responses(&[]);
        let mut lodestar = client_with_canned_responses(&[]);
        let response = handle(
            &mut mindleak,
            &mut lodestar,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }),
        )
        .expect("initialize replies");
        assert_eq!(
            response["result"]["serverInfo"]["name"],
            "mindleak-coordinator"
        );
    }

    #[test]
    fn tools_list_advertises_both_composed_tools() {
        let mut mindleak = client_with_canned_responses(&[]);
        let mut lodestar = client_with_canned_responses(&[]);
        let response = handle(
            &mut mindleak,
            &mut lodestar,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
        )
        .expect("tools/list replies");
        let names: Vec<&str> = response["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec!["coordinator_open_session", "coordinator_preflight"]
        );
    }

    #[test]
    fn tools_call_dispatches_open_session_and_wraps_the_result() {
        let mut mindleak = client_with_canned_responses(&[
            tool_result(json!({ "agent_id": "a" })),
            tool_result(json!({ "repository_id": "r" })),
        ]);
        let mut lodestar = client_with_canned_responses(&[
            tool_result(json!({ "agent_id": "a" })),
            tool_result(json!({ "repository_id": "r" })),
        ]);
        let response = handle(
            &mut mindleak,
            &mut lodestar,
            &json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "coordinator_open_session", "arguments": { "session_id": "tok" } }
            }),
        )
        .expect("tools/call replies");
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Both planes opened"));
        assert_eq!(
            response["result"]["structuredContent"]["identity"]["agents_match"],
            true
        );
    }

    #[test]
    fn tools_call_refuses_an_unknown_tool_name() {
        let mut mindleak = client_with_canned_responses(&[]);
        let mut lodestar = client_with_canned_responses(&[]);
        let response = handle(
            &mut mindleak,
            &mut lodestar,
            &json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "bogus", "arguments": {} }
            }),
        )
        .expect("tools/call replies");
        assert_eq!(response["result"]["isError"], true);
    }

    #[test]
    fn unknown_method_with_an_id_returns_a_json_rpc_error() {
        let mut mindleak = client_with_canned_responses(&[]);
        let mut lodestar = client_with_canned_responses(&[]);
        let response = handle(
            &mut mindleak,
            &mut lodestar,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "bogus" }),
        )
        .expect("errors still get a response");
        assert_eq!(response["error"]["code"], -32601);
    }

    #[test]
    fn a_notification_with_no_id_gets_no_response() {
        let mut mindleak = client_with_canned_responses(&[]);
        let mut lodestar = client_with_canned_responses(&[]);
        let response = handle(
            &mut mindleak,
            &mut lodestar,
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        );
        assert!(response.is_none());
    }
}
