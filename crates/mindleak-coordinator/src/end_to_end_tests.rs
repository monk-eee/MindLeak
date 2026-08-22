//! Real end-to-end verification against the actual spawned binaries. Skips
//! (does not fail) when they are not built next to this workspace's
//! `target/`, mirroring the Postgres-gated-test pattern in `ackplane-server`:
//! this needs a real dependency, not a fake, but a fresh checkout with no
//! release build yet must not fail CI over it.
//!
//! Runs both children against a throwaway git repository under the OS temp
//! directory rather than this checkout, so it resolves its own isolated
//! `repository_id` instead of writing into the real shared `graph.db`/
//! `spec.db` every worktree of this clone otherwise shares.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::child::SpawnedChild;
use crate::tools;

fn discover_binary(env_var: &str, name: &str) -> Option<PathBuf> {
    if let Ok(configured) = std::env::var(env_var) {
        let path = PathBuf::from(configured);
        if path.is_file() {
            return Some(path);
        }
    }
    let exe_name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for profile in ["debug", "release"] {
        let candidate = manifest_dir
            .join("..")
            .join("..")
            .join("target")
            .join(profile)
            .join(&exe_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// A throwaway git repository under the OS temp directory, cleaned up on
/// drop, so the real spawned servers resolve an isolated `repository_id`
/// instead of this checkout's shared one.
struct ScratchRepo {
    path: PathBuf,
}

impl ScratchRepo {
    fn create() -> Self {
        let unique = format!(
            "mindleak-coordinator-e2e-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).expect("create scratch repo dir");
        let status = Command::new("git")
            .arg("init")
            .arg("--quiet")
            .current_dir(&path)
            .status()
            .expect("run git init");
        assert!(status.success(), "git init failed in scratch repo");
        Self { path }
    }
}

impl Drop for ScratchRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn coordinator_open_session_and_preflight_work_against_the_real_binaries() {
    let Some(mindleak_bin) = discover_binary("MINDLEAK_MCP_BIN", "mindleak-mcp") else {
        eprintln!(
            "skipped: mindleak-mcp binary not found (build with `cargo build -p mindleak-mcp`)"
        );
        return;
    };
    let Some(lodestar_bin) = discover_binary("LODESTAR_MCP_BIN", "lodestar-mcp") else {
        eprintln!(
            "skipped: lodestar-mcp binary not found (build with `cargo build -p lodestar-mcp`)"
        );
        return;
    };

    let scratch = ScratchRepo::create();
    let mindleak_bin = mindleak_bin.display().to_string();
    let lodestar_bin = lodestar_bin.display().to_string();
    let mut mindleak =
        SpawnedChild::spawn_in(&mindleak_bin, &scratch.path).expect("spawn mindleak-mcp");
    let mut lodestar =
        SpawnedChild::spawn_in(&lodestar_bin, &scratch.path).expect("spawn lodestar-mcp");
    mindleak.client.initialize().expect("mindleak initialize");
    lodestar.client.initialize().expect("lodestar initialize");

    let session_id = "0123456789abcdef0123456789abcdef";
    let composed = tools::open_session(
        &mut mindleak.client,
        &mut lodestar.client,
        serde_json::json!({ "session_id": session_id, "branch": "test/coordinator-e2e" }),
    );

    assert_eq!(composed["both_open"], true, "{composed:#}");
    assert_eq!(
        composed["identity"]["agents_match"], true,
        "the real servers must resolve the same agent_id for one session_id: {composed:#}"
    );

    let preflight = tools::preflight(
        &mut mindleak.client,
        &mut lodestar.client,
        session_id,
        &["crates/mindleak-coordinator/src/main.rs".to_string()],
        &[],
        None,
    );
    assert_eq!(preflight["all_answered"], true, "{preflight:#}");
}
