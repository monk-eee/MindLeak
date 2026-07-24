//! Lodestar MCP — an MCP (Model Context Protocol) server over stdio that exposes
//! the Intent Plane (durable constitution, task coordination, conformance, and
//! consolidated knowledge) to coding agents.

mod server;
mod tools;

use std::path::{Path, PathBuf};

use lodestar_core::Lodestar;

fn main() -> anyhow::Result<()> {
    let db_path = resolve_db_path();
    if let Some(parent) = Path::new(&db_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let agent = resolve_agent_identity(
        std::env::var("LODESTAR_AGENT_ID").ok(),
        std::env::var("LODESTAR_AGENT").ok(),
        &process_nonce(),
    );
    if let Some(id) = &agent {
        eprintln!("[lodestar-mcp] agent = {id}");
    }
    let engine = Lodestar::open(&db_path)?.with_agent(agent);
    eprintln!("[lodestar-mcp] ready — intent plane at {db_path}");
    server::run(engine)
}

/// Resolve the spec database path.
///
/// Order: an explicit `LODESTAR_DB` override wins; otherwise the Intent Plane
/// lives at the git repository root (the parent of the git *common* dir) so
/// every worktree of a repo shares one plane by default (ADR-0018); with no git
/// repository it falls back to `<cwd>/.lodestar/spec.db`.
fn resolve_db_path() -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let env_override = std::env::var("LODESTAR_DB").ok();
    let common_dir = git_common_dir(&cwd);
    resolve_db_path_from(env_override.as_deref(), common_dir.as_deref(), &cwd)
        .to_string_lossy()
        .into_owned()
}

/// Pure resolution used by [`resolve_db_path`], with every input injected so the
/// three cases (override / git repo / no git) are deterministically testable.
fn resolve_db_path_from(
    env_override: Option<&str>,
    git_common_dir: Option<&Path>,
    cwd: &Path,
) -> PathBuf {
    if let Some(value) = env_override {
        if !value.trim().is_empty() {
            return PathBuf::from(value);
        }
    }
    // The repo root is the parent of the common `.git` dir; a linked worktree's
    // common dir points back at the main repo, so all worktrees share one root.
    let root = git_common_dir
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| cwd.to_path_buf());
    root.join(".lodestar").join("spec.db")
}

/// The git *common* directory for `cwd` (`git rev-parse --git-common-dir`),
/// resolved to an absolute path. `None` when `cwd` is not inside a git repo or
/// git is unavailable — callers then fall back to the current directory.
fn git_common_dir(cwd: &Path) -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8(output.stdout).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = Path::new(trimmed);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    // Canonicalize so a relative `../.git` from a subdir collapses to the real
    // root before we take its parent; keep the joined path if that fails.
    Some(absolute.canonicalize().unwrap_or(absolute))
}

/// Resolve this process's agent identity (ADR-0030). An explicit `LODESTAR_AGENT_ID`
/// pin is used verbatim; otherwise a configured `LODESTAR_AGENT` base label is made
/// unique per process as `<base>-<nonce>`, so concurrent agents never alias onto one
/// id; with neither set, task ownership/attribution stays off. Pure over its inputs
/// so resolution is unit-tested - `main` injects the real env values and a nonce.
fn resolve_agent_identity(
    pin: Option<String>,
    base: Option<String>,
    nonce: &str,
) -> Option<String> {
    let cleaned = |value: Option<String>| {
        value
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    if let Some(id) = cleaned(pin) {
        return Some(id);
    }
    cleaned(base).map(|b| format!("{b}-{nonce}"))
}

/// A short, process-unique nonce (8 hex) from the pid and start time - enough to
/// tell concurrent sessions apart without a crypto dependency (ADR-0030).
fn process_nonce() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mixed = nanos ^ (std::process::id() as u64).rotate_left(32);
    format!("{:08x}", mixed as u32)
}

#[cfg(test)]
mod tests {
    use super::{process_nonce, resolve_agent_identity, resolve_db_path_from};
    use std::path::{Path, PathBuf};

    #[test]
    fn agent_pin_is_used_verbatim() {
        assert_eq!(
            resolve_agent_identity(Some("fixed-ci".into()), Some("copilot".into()), "abcd1234"),
            Some("fixed-ci".into())
        );
    }

    #[test]
    fn agent_base_is_made_unique_per_process() {
        assert_eq!(
            resolve_agent_identity(None, Some("copilot".into()), "abcd1234"),
            Some("copilot-abcd1234".into())
        );
    }

    #[test]
    fn distinct_nonces_yield_distinct_agent_ids() {
        assert_ne!(
            resolve_agent_identity(None, Some("copilot".into()), "aaaa1111"),
            resolve_agent_identity(None, Some("copilot".into()), "bbbb2222")
        );
    }

    #[test]
    fn agent_off_when_neither_pin_nor_base_set() {
        assert_eq!(resolve_agent_identity(None, None, "abcd1234"), None);
        assert_eq!(
            resolve_agent_identity(Some("  ".into()), Some("  ".into()), "abcd1234"),
            None
        );
    }

    #[test]
    fn process_nonce_is_eight_hex_chars() {
        let nonce = process_nonce();
        assert_eq!(nonce.len(), 8);
        assert!(nonce.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn lodestar_db_override_wins_over_git_and_cwd() {
        let got = resolve_db_path_from(
            Some("/custom/plane.db"),
            Some(Path::new("/repo/.git")),
            Path::new("/repo/worktree"),
        );
        assert_eq!(got, PathBuf::from("/custom/plane.db"));
    }

    #[test]
    fn a_blank_override_is_ignored() {
        let got = resolve_db_path_from(Some("   "), None, Path::new("/here"));
        assert_eq!(got, Path::new("/here").join(".lodestar").join("spec.db"));
    }

    #[test]
    fn worktrees_of_one_repo_share_a_single_plane_at_the_repo_root() {
        // A linked worktree's cwd differs from the main checkout, but the git
        // common dir points back at the main `.git`, so both resolve identically.
        let expected = Path::new("/repo").join(".lodestar").join("spec.db");
        let main_checkout =
            resolve_db_path_from(None, Some(Path::new("/repo/.git")), Path::new("/repo"));
        let linked_worktree = resolve_db_path_from(
            None,
            Some(Path::new("/repo/.git")),
            Path::new("/repo-feature"),
        );
        assert_eq!(main_checkout, expected);
        assert_eq!(linked_worktree, expected);
    }

    #[test]
    fn falls_back_to_cwd_outside_a_git_repo() {
        let got = resolve_db_path_from(None, None, Path::new("/tmp/scratch"));
        assert_eq!(
            got,
            Path::new("/tmp/scratch").join(".lodestar").join("spec.db")
        );
    }
}
