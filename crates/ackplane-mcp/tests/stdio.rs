//! Drives the built binary over real stdio, one JSON object per line -- the
//! same way an MCP client does. The unit tests exercise the dispatch function;
//! only this proves the process itself speaks the protocol on its own stdin and
//! stdout, which is the part a client actually depends on.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

/// Send `requests` to a fresh server process and collect one response per line.
fn exchange(environment: &[(&str, &str)], requests: &[Value]) -> Vec<Value> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ackplane-mcp"));
    command
        .env_remove("ACKPLANE_MCP_ENDPOINT")
        .env_remove("ACKPLANE_NODE_KEY_PATH")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (key, value) in environment {
        command.env(key, value);
    }
    let mut child = command.spawn().expect("the server binary starts");

    {
        let stdin = child.stdin.as_mut().expect("stdin is piped");
        for request in requests {
            writeln!(stdin, "{request}").expect("a request is written");
        }
    }
    // Dropping stdin is the EOF that ends the server's loop.
    drop(child.stdin.take());

    let stdout = child.stdout.take().expect("stdout is piped");
    let responses: Vec<Value> = BufReader::new(stdout)
        .lines()
        .map(|line| line.expect("a readable line"))
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(&line).expect("each line is one JSON object"))
        .collect();
    child.wait().expect("the server exits cleanly");
    responses
}

fn request(id: i64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

#[test]
fn the_server_completes_a_handshake_and_advertises_its_tool_over_stdio() {
    let responses = exchange(
        &[("ACKPLANE_MCP_ENDPOINT", "http://127.0.0.1:8443")],
        &[
            request(1, "initialize", json!({})),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            request(2, "tools/list", json!({})),
        ],
    );

    // The notification earns no response, so two requests earn two lines.
    assert_eq!(responses.len(), 2, "got: {responses:?}");
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"]["serverInfo"]["name"], "ackplane-mcp");
    assert_eq!(responses[1]["id"], 2);
    assert_eq!(responses[1]["result"]["tools"][0]["name"], "open_session");
}

/// ADR-0137 clause 2 end to end: `open_session` works over the real process's
/// stdio even with no Ackplane endpoint reachable, because it never dials one.
#[test]
fn open_session_returns_a_real_agent_id_over_stdio() {
    let responses = exchange(
        &[("ACKPLANE_MCP_ENDPOINT", "http://127.0.0.1:8443")],
        &[request(
            1,
            "tools/call",
            json!({
                "name": "open_session",
                "arguments": { "session_id": "0123456789abcdef0123456789abcdef" }
            }),
        )],
    );

    assert_eq!(responses.len(), 1, "got: {responses:?}");
    assert_eq!(responses[0]["result"]["isError"], false);
    let content: Value = responses[0]["result"]["content"][0]["text"]
        .as_str()
        .and_then(|text| serde_json::from_str(text).ok())
        .expect("open_session's content is a JSON body");
    assert!(
        content["agent_id"]
            .as_str()
            .expect("agent_id is a string")
            .starts_with("session:v1:"),
        "got: {content}"
    );
}

/// ADR-0136 clause 4 end to end: a remote endpoint must not be dialled, and the
/// process must still answer the protocol so the agent can read why.
#[test]
fn a_remote_endpoint_is_refused_by_the_running_process_not_merely_by_a_pure_function() {
    let responses = exchange(
        &[("ACKPLANE_MCP_ENDPOINT", "https://ackplane.example.com:8443")],
        &[
            request(1, "initialize", json!({})),
            request(
                2,
                "tools/call",
                json!({ "name": "check_enrollment_status", "arguments": {} }),
            ),
        ],
    );

    assert_eq!(responses.len(), 2, "got: {responses:?}");
    let instructions = responses[0]["result"]["instructions"]
        .as_str()
        .expect("a refused server explains itself at initialize");
    assert!(instructions.contains("ADR-0136 clause 4"), "{instructions}");

    assert_eq!(responses[1]["result"]["isError"], true);
    let refusal = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("the refusal reaches the agent as a tool result");
    assert!(
        refusal.contains("ackplane.example.com"),
        "the refusal names the host it refused: {refusal}"
    );
}

/// A malformed line must not kill a long-lived server: the next request on the
/// same connection still has to be answered.
#[test]
fn a_malformed_line_is_reported_and_the_server_keeps_serving() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ackplane-mcp"));
    command
        .env("ACKPLANE_MCP_ENDPOINT", "http://127.0.0.1:8443")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().expect("the server binary starts");
    {
        let stdin = child.stdin.as_mut().expect("stdin is piped");
        writeln!(stdin, "{{ not json").expect("a malformed line is written");
        writeln!(stdin, "{}", request(9, "ping", json!({}))).expect("a valid line is written");
    }
    drop(child.stdin.take());

    let stdout = child.stdout.take().expect("stdout is piped");
    let responses: Vec<Value> = BufReader::new(stdout)
        .lines()
        .map(|line| serde_json::from_str(&line.expect("a readable line")).expect("JSON"))
        .collect();
    child.wait().expect("the server exits cleanly");

    assert_eq!(responses.len(), 2, "got: {responses:?}");
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[1]["id"], 9);
}
