use std::{process::Stdio, time::Duration};

use serde_json::{json, Value};
use tokio::{io::AsyncWriteExt, process::Command};

use super::{fixture::Fixture, API_KEY};

pub struct ToolReply {
    pub is_error: bool,
    pub progress: Value,
}

impl Fixture {
    pub async fn index(&self, model_url: &str, model: &str, arguments: &[Value]) -> Vec<ToolReply> {
        let output = tokio::time::timeout(Duration::from_secs(40), async {
            let mut child = Command::new(env!("CARGO_BIN_EXE_ackplane-mcp"))
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .env_clear()
                .env("ACKPLANE_MCP_ENDPOINT", &self.endpoint)
                .env("MINDLEAK_ACKPLANE_STATE_DIR", self.directory.path())
                .env("MINDLEAK_ACKPLANE_TENANT_ID", &self.tenant_id)
                .env("MINDLEAK_ACKPLANE_REPOSITORY_ID", &self.repository_id)
                .env("MINDLEAK_EMBED_URL", model_url)
                .env("MINDLEAK_EMBED_MODEL", model)
                .env("MINDLEAK_EMBED_API_KEY", API_KEY)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .expect("spawn the real ackplane-mcp binary");
            let mut stdin = child.stdin.take().unwrap();
            for (index, arguments) in arguments.iter().enumerate() {
                let request = json!({
                    "jsonrpc": "2.0", "id": index, "method": "tools/call",
                    "params": {"name": "index", "arguments": arguments}
                });
                stdin
                    .write_all(format!("{request}\n").as_bytes())
                    .await
                    .unwrap();
            }
            drop(stdin);
            child
                .wait_with_output()
                .await
                .expect("wait for the MCP stdout to close")
        })
        .await
        .expect("the real MCP child must exit within 40 seconds");
        assert!(
            output.status.success(),
            "MCP process exit status: {}",
            output.status
        );
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains(API_KEY),
            "stderr must not expose authorization"
        );
        let stdout = String::from_utf8(output.stdout).expect("MCP stdout is UTF-8");
        assert!(
            !stdout.contains(API_KEY),
            "MCP results must not expose authorization"
        );
        let replies: Vec<_> = stdout
            .lines()
            .enumerate()
            .map(|(index, line)| {
                let response: Value = serde_json::from_str(line).expect("MCP emits JSON-RPC lines");
                assert_eq!(response["jsonrpc"], "2.0");
                assert_eq!(response["id"], index);
                assert!(
                    response.get("error").is_none(),
                    "index must return an MCP tool result"
                );
                ToolReply {
                    is_error: response["result"]["isError"]
                        .as_bool()
                        .expect("tool error indicator"),
                    progress: serde_json::from_str(
                        response["result"]["content"][0]["text"]
                            .as_str()
                            .expect("tool result has text content"),
                    )
                    .expect("index reports structured progress"),
                }
            })
            .collect();
        assert_eq!(
            replies.len(),
            arguments.len(),
            "every index request must receive a reply"
        );
        replies
    }
}
