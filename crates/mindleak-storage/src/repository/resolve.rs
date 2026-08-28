//! Which database a plane resolves to, and where its state root lives.

use std::io;
use std::path::{Path, PathBuf};

use super::fs::{git_command, nonblank_path};
use super::identity::repository_id;
use super::platform::{platform_state_root_from, Platform};
use super::{DatabaseKind, DatabaseOrigin, RepositoryStorageError, ResolvedDatabase};

/// Resolve one plane's database. An explicit database path wins. Inside a Git
/// clone, all linked worktrees resolve through the shared repository id. Outside
/// Git, scratch use stays workspace-local.
pub fn resolve_database(
    workspace: &Path,
    kind: DatabaseKind,
    explicit_database: Option<&str>,
) -> Result<ResolvedDatabase, RepositoryStorageError> {
    if let Some(path) = nonblank_path(explicit_database) {
        return Ok(explicit_database_resolution(path));
    }
    ensure_workspace_exists(workspace)?;
    let Some(common_dir) = git_common_dir(workspace)? else {
        return Ok(workspace_database_resolution(workspace, kind));
    };
    let state_root = platform_state_root()?;
    resolve_repository_database(workspace, &common_dir, kind, &state_root)
}

/// Inject the local state root while retaining real Git discovery. This is also
/// the deterministic seam used by tests and managed/container launchers.
pub fn resolve_database_in(
    workspace: &Path,
    kind: DatabaseKind,
    explicit_database: Option<&str>,
    state_root: &Path,
) -> Result<ResolvedDatabase, RepositoryStorageError> {
    if let Some(path) = nonblank_path(explicit_database) {
        return Ok(explicit_database_resolution(path));
    }
    ensure_workspace_exists(workspace)?;
    let Some(common_dir) = git_common_dir(workspace)? else {
        return Ok(workspace_database_resolution(workspace, kind));
    };
    resolve_repository_database(workspace, &common_dir, kind, state_root)
}

/// A workspace that is not there is a stale caller argument, not a directory
/// that happens to sit outside Git. Both make `git rev-parse` unusable, but only
/// the latter may fall back to a workspace-local database: on Unix the failed
/// spawn reports `NotFound`, indistinguishable from a missing `git` binary, so
/// without this guard a reclaimed worktree silently resolves to a fresh empty
/// database (Windows reports `NotADirectory` and surfaced an opaque IO error).
fn ensure_workspace_exists(workspace: &Path) -> Result<(), RepositoryStorageError> {
    if workspace.is_dir() {
        return Ok(());
    }
    Err(RepositoryStorageError::MissingWorkspace(
        workspace.to_path_buf(),
    ))
}

pub fn platform_state_root() -> Result<PathBuf, RepositoryStorageError> {
    if let Some(configured) = nonblank_path(std::env::var("MINDLEAK_HOME").ok().as_deref()) {
        return Ok(PathBuf::from(configured));
    }
    platform_state_root_from(|name| std::env::var(name).ok(), Platform::current())
}

/// Resolve a caller-declared worktree path relative to the process directory.
/// Clients pass this same path to both planes when their spawn CWD is not the
/// repository worktree (for example Copilot CLI or desktop MCP clients).
pub fn resolve_workspace_path(current: &Path, configured: Option<&str>) -> PathBuf {
    let Some(configured) = nonblank_path(configured) else {
        return current.to_path_buf();
    };
    let configured = PathBuf::from(configured);
    if configured.is_absolute() {
        configured
    } else {
        current.join(configured)
    }
}

fn resolve_repository_database(
    workspace: &Path,
    common_dir: &Path,
    kind: DatabaseKind,
    state_root: &Path,
) -> Result<ResolvedDatabase, RepositoryStorageError> {
    let repository_id = repository_id(workspace, common_dir)?;
    let repository_root = common_dir
        .parent()
        .ok_or_else(|| RepositoryStorageError::Git("git common dir has no parent".to_string()))?;
    let directory = state_root.join("repositories").join(&repository_id);
    Ok(ResolvedDatabase {
        path: directory.join(kind.file_name()),
        origin: DatabaseOrigin::Repository,
        repository_id: Some(repository_id),
        legacy_path: Some(repository_root.join(kind.legacy_relative_path())),
    })
}

pub(crate) fn explicit_database_resolution(path: &str) -> ResolvedDatabase {
    ResolvedDatabase {
        path: PathBuf::from(path),
        origin: DatabaseOrigin::Explicit,
        repository_id: None,
        legacy_path: None,
    }
}

fn workspace_database_resolution(workspace: &Path, kind: DatabaseKind) -> ResolvedDatabase {
    ResolvedDatabase {
        path: workspace.join(kind.legacy_relative_path()),
        origin: DatabaseOrigin::Workspace,
        repository_id: None,
        legacy_path: None,
    }
}

fn git_common_dir(workspace: &Path) -> Result<Option<PathBuf>, RepositoryStorageError> {
    let output = match git_command()
        .args(["rev-parse", "--git-common-dir"])
        .current_dir(workspace)
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !output.status.success() {
        return Ok(None);
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    let value = raw.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let path = Path::new(value);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    };
    Ok(Some(absolute.canonicalize().unwrap_or(absolute)))
}
