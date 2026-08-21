//! A minimal newline-delimited JSON-RPC 2.0 client, generic over its transport
//! so it is unit-testable with in-memory streams and needs no real subprocess
//! (ADR-0097 decision 1: a thin client/gateway over the existing stdio
//! servers, not a third source of graph or intent truth).

use std::io::{BufRead, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

/// One stdio JSON-RPC connection. `R`/`W` are generic so tests can inject a
/// `Cursor`/`Vec<u8>` pair instead of spawning a real process (mirroring how
/// `clients/node/mindleak-client`'s `McpConnection` is tested against
/// injected `PassThrough` streams rather than a subprocess).
pub struct ChildClient<R, W> {
    reader: R,
    writer: W,
    next_id: u64,
}

impl<R: BufRead, W: Write> ChildClient<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader,
            writer,
            next_id: 1,
        }
    }

    /// Send one JSON-RPC request and return its `result`, or a plain-text
    /// error covering a transport failure, a JSON-RPC `error`, or a malformed
    /// reply — the caller never has to distinguish those to report a failed
    /// plane.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let mut line = serde_json::to_string(&request)
            .map_err(|e| format!("{method}: encode request: {e}"))?;
        line.push('\n');
        self.writer
            .write_all(line.as_bytes())
            .map_err(|e| format!("{method}: write request: {e}"))?;
        self.writer
            .flush()
            .map_err(|e| format!("{method}: flush request: {e}"))?;

        let mut response_line = String::new();
        let read = self
            .reader
            .read_line(&mut response_line)
            .map_err(|e| format!("{method}: read response: {e}"))?;
        if read == 0 {
            return Err(format!(
                "{method}: child closed its output before answering"
            ));
        }
        let response: Value = serde_json::from_str(response_line.trim())
            .map_err(|e| format!("{method}: malformed response: {e}"))?;
        if let Some(error) = response.get("error") {
            return Err(format!("{method}: {error}"));
        }
        response
            .get("result")
            .cloned()
            .ok_or_else(|| format!("{method}: response had neither result nor error"))
    }

    /// The MCP handshake every server expects before anything else.
    pub fn initialize(&mut self) -> Result<Value, String> {
        self.call("initialize", json!({}))
    }

    /// Call one tool and return its payload, preferring `structuredContent`
    /// (ADR-0027) and falling back to parsing `content[0].text` as JSON, then
    /// to the raw text for a tool that legitimately answers in prose.
    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        let result = self.call(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )?;
        if let Some(structured) = result.get("structuredContent") {
            return Ok(structured.clone());
        }
        let text = result
            .get("content")
            .and_then(|content| content.get(0))
            .and_then(|first| first.get("text"))
            .and_then(Value::as_str);
        match text {
            Some(text) => {
                Ok(serde_json::from_str(text).unwrap_or_else(|_| json!({ "text": text })))
            }
            None => Err(format!("{name}: tool result carried no content")),
        }
    }

    /// Test-only escape hatch: hand back the raw writer so a test can inspect
    /// exactly what was sent, without exposing it as a general accessor.
    #[cfg(test)]
    pub(crate) fn into_writer(self) -> W {
        self.writer
    }
}

/// A spawned child process plus the client talking to it over its piped
/// stdio. Killing it on drop keeps a coordinator crash or a dropped test
/// fixture from leaking an orphaned server process.
pub struct SpawnedChild {
    child: Child,
    pub client: ChildClient<std::io::BufReader<ChildStdout>, ChildStdin>,
}

impl SpawnedChild {
    pub fn spawn(binary: &str) -> std::io::Result<Self> {
        Self::spawn_with(Command::new(binary))
    }

    /// Used by the end-to-end test to run a child against an isolated
    /// workspace directory rather than whatever repository the test happens
    /// to be checked out in.
    #[cfg(test)]
    pub(crate) fn spawn_in(binary: &str, current_dir: &std::path::Path) -> std::io::Result<Self> {
        let mut command = Command::new(binary);
        command.current_dir(current_dir);
        Self::spawn_with(command)
    }

    fn spawn_with(mut command: Command) -> std::io::Result<Self> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let client = ChildClient::new(std::io::BufReader::new(stdout), stdin);
        Ok(Self { child, client })
    }
}

impl Drop for SpawnedChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn client_with_canned_response(response: &str) -> ChildClient<Cursor<Vec<u8>>, Vec<u8>> {
        let mut bytes = response.as_bytes().to_vec();
        bytes.push(b'\n');
        ChildClient::new(Cursor::new(bytes), Vec::new())
    }

    #[test]
    fn call_writes_one_newline_delimited_json_rpc_request() {
        let mut client = client_with_canned_response(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
        client.call("ping", json!({})).expect("call succeeds");

        let written = String::from_utf8(client.writer.clone()).expect("utf8 request");
        assert_eq!(written.matches('\n').count(), 1, "exactly one line written");
        let sent: Value = serde_json::from_str(written.trim()).expect("valid json request");
        assert_eq!(sent["method"], "ping");
        assert_eq!(sent["id"], 1);
    }

    #[test]
    fn call_returns_the_result_field() {
        let mut client =
            client_with_canned_response(r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#);
        let result = client.call("ping", json!({})).expect("call succeeds");
        assert_eq!(result, json!({ "ok": true }));
    }

    #[test]
    fn call_surfaces_a_json_rpc_error_as_a_plain_message() {
        let mut client = client_with_canned_response(
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"method not found"}}"#,
        );
        let error = client.call("bogus", json!({})).expect_err("call fails");
        assert!(error.contains("method not found"), "{error}");
    }

    #[test]
    fn call_reports_eof_as_a_plane_failure_not_a_hang() {
        let mut client = ChildClient::new(Cursor::new(Vec::new()), Vec::new());
        let error = client.call("ping", json!({})).expect_err("call fails");
        assert!(error.contains("closed its output"), "{error}");
    }

    #[test]
    fn call_reports_malformed_json_rather_than_panicking() {
        let mut client = client_with_canned_response("not json");
        let error = client.call("ping", json!({})).expect_err("call fails");
        assert!(error.contains("malformed response"), "{error}");
    }

    #[test]
    fn call_tool_prefers_structured_content() {
        let mut client = client_with_canned_response(
            r#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"ignored"}],"structuredContent":{"agent_id":"a"}}}"#,
        );
        let value = client
            .call_tool("open_session", json!({}))
            .expect("call succeeds");
        assert_eq!(value, json!({ "agent_id": "a" }));
    }

    #[test]
    fn call_tool_falls_back_to_parsing_content_text_as_json() {
        let mut client = client_with_canned_response(
            r#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"{\"agent_id\":\"a\"}"}]}}"#,
        );
        let value = client
            .call_tool("open_session", json!({}))
            .expect("call succeeds");
        assert_eq!(value, json!({ "agent_id": "a" }));
    }

    #[test]
    fn call_tool_falls_back_to_raw_text_for_a_prose_only_reply() {
        let mut client = client_with_canned_response(
            r#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"plain prose"}]}}"#,
        );
        let value = client
            .call_tool("some_tool", json!({}))
            .expect("call succeeds");
        assert_eq!(value, json!({ "text": "plain prose" }));
    }

    #[test]
    fn ids_increment_across_calls_so_replies_could_be_correlated() {
        let mut client = client_with_canned_response(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
        client.call("first", json!({})).expect("first call");
        // The canned reader only has one line, so a second read hits EOF —
        // proving the id still advanced is the point of this test, not a
        // second successful round trip.
        let _ = client.call("second", json!({}));
        let written = String::from_utf8(client.writer.clone()).expect("utf8 requests");
        let ids: Vec<Value> = written
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("valid json")["id"].clone())
            .collect();
        assert_eq!(ids, vec![json!(1), json!(2)]);
    }
}
