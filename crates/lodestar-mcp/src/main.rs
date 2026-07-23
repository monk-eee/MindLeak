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
    let agent = std::env::var("LODESTAR_AGENT")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let engine = Lodestar::open(&db_path)?.with_agent(agent);
    // Binding hygiene on every restart (restarts are frequent — VS Code updates,
    // reloads): goals govern code, so drop any stale documentation binding that
    // would otherwise make unrelated commits drift. Safe and idempotent.
    match engine.prune_ungovernable_bindings() {
        Ok(0) => {}
        Ok(pruned) => {
            eprintln!("[lodestar-mcp] binding hygiene: pruned {pruned} ungovernable documentation binding(s)");
        }
        Err(error) => eprintln!("[lodestar-mcp] binding hygiene skipped: {error}"),
    }
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

#[cfg(test)]
mod tests {
    use super::resolve_db_path_from;
    use std::path::{Path, PathBuf};

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
