use std::path::Path;

use mindleak_core::{ingest::git::CommitRecord, MindLeak, MindLeakError};
use serde_json::{json, Value};

use crate::tools::{refused, req_str, text_result};

pub(super) fn definition() -> Value {
    json!({
        "name": "repair_commit_attribution",
        "description": "Explicitly correct one existing commit's stored attribution from the configured repository's exact Git object. Retains a durable, session-attributed before/after audit in telemetry; preserves unrelated history and original authorship. Refuses if Git cannot verify the facts. Does not accept replacement files, messages or timestamps, and does not certify a task.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "sha": { "type": "string", "description": "Full 40- or 64-character commit hash." },
                "reason": { "type": "string", "minLength": 1, "maxLength": 2048 }
            },
            "required": ["sha", "reason"]
        }
    })
}

pub(super) fn dispatch(engine: &MindLeak, args: &Value) -> Result<Value, String> {
    let agent = req_str(args, "agent")?;
    let sha = req_str(args, "sha")?;
    let reason = req_str(args, "reason")?;
    let result = engine.repair_commit_attribution_for_agent(&agent, &sha, &reason, |root, hash| {
        let facts = mindleak_storage::read_commit(Path::new(root), hash)?;
        Ok(CommitRecord {
            sha: Some(facts.sha),
            message: facts.message,
            changed_files: facts.changed_files,
            timestamp: facts.timestamp,
        })
    });
    let outcome = result.map_err(|error| match error {
        MindLeakError::InvalidArgument(_) => refused(error),
        _ => error.to_string(),
    })?;
    Ok(text_result(&json!(outcome)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{bind_session, call, tests::content_text};
    use mindleak_core::{Direction, RelationType};
    use mindleak_session::{SessionContext, SessionRegistry};
    use std::path::PathBuf;
    use std::process::Command;

    const SESSION: &str = "0123456789abcdef0123456789abcdef";

    struct Repository(PathBuf);

    impl Repository {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "mindleak-repair-mcp-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            let repo = Self(path);
            repo.git(&["init", "--quiet", "-b", "main"]);
            repo.git(&["config", "user.name", "Repair Test"]);
            repo.git(&["config", "user.email", "repair@example.invalid"]);
            std::fs::write(repo.0.join("true.txt"), "true\n").unwrap();
            repo.git(&["add", "true.txt"]);
            repo.git(&["commit", "--quiet", "-m", "fix: true work"]);
            repo
        }

        fn git(&self, args: &[&str]) -> String {
            let mut command = Command::new("git");
            for (name, _) in std::env::vars_os() {
                if name.to_string_lossy().starts_with("GIT_") {
                    command.env_remove(name);
                }
            }
            let output = command.args(args).current_dir(&self.0).output().unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        }
    }

    impl Drop for Repository {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn repair_tool_verifies_git_and_audits_the_registered_session() {
        let repo = Repository::new();
        let sha = repo.git(&["rev-parse", "HEAD"]);
        let engine = MindLeak::open_in_memory()
            .unwrap()
            .with_workspace_root(repo.0.to_string_lossy());
        engine
            .ingest_commit_for_agent(
                "original-author",
                &CommitRecord {
                    sha: Some(sha.clone()),
                    message: "wrong message".into(),
                    changed_files: vec!["false.txt".into()],
                    timestamp: 1,
                },
            )
            .unwrap();
        let sessions = SessionRegistry::new("test").unwrap();
        let identity = sessions
            .open_session(SESSION, SessionContext::default())
            .unwrap();
        let params = json!({ "name": "repair_commit_attribution", "arguments": {
            "session_id": SESSION, "sha": sha, "reason": "publication used branch scope", "agent": "forged-author"
        }});
        let bound = bind_session(&params, &sessions).unwrap();
        let response = call(&engine, &bound).unwrap();
        let body: Value = serde_json::from_str(&content_text(&response)).unwrap();
        assert_eq!(body["removed_edges"], 1);
        assert_eq!(body["added_edges"], 1);
        assert!(body["audit_id"].is_i64());
        let graph = engine
            .store()
            .traverse(
                &[format!("intent:{sha}")],
                Direction::Outgoing,
                1,
                0.0,
                mindleak_core::now_unix(),
            )
            .unwrap();
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].relation, RelationType::Refactored);
        assert_eq!(graph.edges[0].target_id, "artifact:true.txt");
        let audit = engine
            .telemetry_snapshot(10)
            .unwrap()
            .recent
            .into_iter()
            .find(|event| event.kind == "evidence_repair")
            .unwrap()
            .detail
            .unwrap();
        assert_eq!(audit["agent_id"], identity.agent_id);
        assert_eq!(audit["before"]["node"]["created_at"], 1);
        assert_eq!(audit["verified"]["message"], "fix: true work");
        assert!(engine
            .store()
            .get_node(&format!("agent:{}", identity.agent_id))
            .unwrap()
            .is_none());
        let repeat: Value =
            serde_json::from_str(&content_text(&call(&engine, &bound).unwrap())).unwrap();
        assert!(repeat["audit_id"].is_null());
    }

    #[test]
    fn repair_tool_rejects_unregistered_sessions_and_caller_supplied_facts() {
        let sessions = SessionRegistry::new("test").unwrap();
        let params = json!({ "name": "repair_commit_attribution", "arguments": {
            "session_id": SESSION, "sha": "a".repeat(40), "reason": "reason"
        }});
        assert!(bind_session(&params, &sessions).is_err());
        sessions
            .open_session(SESSION, SessionContext::default())
            .unwrap();
        for (key, value) in [
            ("changed_files", json!([])),
            ("timestamp", json!(100)),
            ("message", json!("invented")),
        ] {
            let mut invalid = params.clone();
            invalid["arguments"][key] = value;
            assert!(bind_session(&invalid, &sessions)
                .unwrap_err()
                .contains("unknown argument"));
        }
        let mut missing = params;
        missing["arguments"]
            .as_object_mut()
            .unwrap()
            .remove("session_id");
        assert!(bind_session(&missing, &sessions)
            .unwrap_err()
            .contains("session_id"));
    }
}
