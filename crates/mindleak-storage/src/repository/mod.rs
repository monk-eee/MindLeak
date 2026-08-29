//! Shared repository identity and user-local state resolution (ADR-0038).

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use thiserror::Error;

use crate::backup_database;

use self::fs::{create_private_directory, sibling_path};
use self::migrate::acquire_migration_lock;

mod commit;
mod fs;
mod identity;
mod migrate;
mod platform;
mod resolve;
mod worktree;

pub use commit::commit_exists;
pub use identity::read_local_git_config;
pub use resolve::{
    platform_state_root, resolve_database, resolve_database_in, resolve_workspace_path,
};
pub use worktree::worktree_roots;

/// Shared retry cadence for the bootstrap and migration spin-waits.
pub(crate) const RETRY_DELAY: Duration = Duration::from_millis(10);

#[derive(Debug, Error)]
pub enum RepositoryStorageError {
    #[error("git command failed: {0}")]
    Git(String),
    /// The workspace path names a directory that is not there. Distinct from a
    /// directory that is simply outside Git, which stays a legitimate fallback.
    #[error("workspace directory does not exist: {0}")]
    MissingWorkspace(PathBuf),
    #[error("repository id must be exactly 32 lowercase hexadecimal characters: {0}")]
    InvalidRepositoryId(String),
    #[error("repository id bootstrap did not complete at {0}")]
    RepositoryIdBusy(PathBuf),
    #[error("database migration did not complete at {0}")]
    MigrationBusy(PathBuf),
    #[error("cannot resolve a platform-local MindLeak state root")]
    MissingStateRoot,
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseKind {
    MindLeak,
    Lodestar,
}

impl DatabaseKind {
    fn file_name(self) -> &'static str {
        match self {
            Self::MindLeak => "graph.db",
            Self::Lodestar => "spec.db",
        }
    }

    fn legacy_relative_path(self) -> &'static str {
        match self {
            Self::MindLeak => ".mindleak/graph.db",
            Self::Lodestar => ".lodestar/spec.db",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseOrigin {
    Explicit,
    Repository,
    Workspace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StorageStatus {
    pub plane: String,
    pub repository_id: Option<String>,
    pub database_path: PathBuf,
    pub origin: DatabaseOrigin,
    pub legacy_path: Option<PathBuf>,
    pub migrated_legacy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDatabase {
    pub path: PathBuf,
    pub origin: DatabaseOrigin,
    pub repository_id: Option<String>,
    pub legacy_path: Option<PathBuf>,
}

impl ResolvedDatabase {
    pub fn status(&self, plane: &str, migrated_legacy: bool) -> StorageStatus {
        StorageStatus {
            plane: plane.to_string(),
            repository_id: self.repository_id.clone(),
            database_path: self.path.clone(),
            origin: self.origin,
            legacy_path: self.legacy_path.clone(),
            migrated_legacy,
        }
    }

    /// Copy a legacy repository-local database into the repository store once.
    /// The source remains untouched; concurrent starts serialize on a sibling
    /// lock and all converge on the same verified destination.
    pub fn migrate_legacy_if_needed(&self) -> Result<bool, RepositoryStorageError> {
        if self.origin != DatabaseOrigin::Repository || self.path.exists() {
            return Ok(false);
        }
        let Some(source_path) = self
            .legacy_path
            .as_ref()
            .filter(|candidate| candidate.exists())
        else {
            return Ok(false);
        };
        let parent = self.path.parent().ok_or_else(|| {
            RepositoryStorageError::Io(io::Error::other("database has no parent"))
        })?;
        create_private_directory(parent)?;

        let lock_path = sibling_path(&self.path, ".migration.lock");
        let Some(lock) = acquire_migration_lock(&lock_path, &self.path)? else {
            return Ok(false);
        };
        if self.path.exists() {
            drop(lock);
            return Ok(false);
        }

        let source = Connection::open_with_flags(source_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let result = backup_database(&source, &self.path).map(|()| true);
        drop(source);
        drop(lock);
        result.map_err(RepositoryStorageError::from)
    }

    /// Create the directory the database lives in, if it has one.
    ///
    /// A path with no directory component — `:memory:`, or a bare `graph.db` —
    /// yields `Some("")` from `parent()`, not `None`. `create_dir_all("")`
    /// short-circuits to `Ok`, but the Unix branch then calls `set_permissions`
    /// on that empty path and gets `ENOENT`, so the server exited at startup on
    /// Linux and macOS while Windows, which has no permissions call, started
    /// fine. Skip the empty parent: there is no directory to create or lock down.
    pub fn ensure_parent(&self) -> Result<(), RepositoryStorageError> {
        match self.path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => create_private_directory(parent)?,
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // Generated by AI (UnitTest MCP)
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    use super::identity::{read_repository_id_marker, REPOSITORY_ID_KEY};
    use super::resolve::explicit_database_resolution;
    use super::*;

    static NEXT_SANDBOX: AtomicUsize = AtomicUsize::new(0);

    struct Sandbox {
        root: PathBuf,
    }

    impl Sandbox {
        fn new(name: &str) -> Self {
            let ordinal = NEXT_SANDBOX.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "mindleak-repository-storage-{name}-{}-{ordinal}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn repository(&self, name: &str) -> PathBuf {
            let repository = self.root.join(name);
            fs::create_dir_all(&repository).unwrap();
            git(&repository, &["init", "-b", "main"]);
            git(&repository, &["config", "user.name", "MindLeak Test"]);
            git(
                &repository,
                &["config", "user.email", "mindleak@example.invalid"],
            );
            fs::write(repository.join("README.md"), "test\n").unwrap();
            git(&repository, &["add", "README.md"]);
            git(&repository, &["commit", "-m", "initial"]);
            repository
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn git(cwd: &Path, args: &[&str]) -> String {
        let output = super::fs::git_command()
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn linked_worktrees_share_one_repository_id_and_database_paths() {
        let sandbox = Sandbox::new("worktrees");
        let repository = sandbox.repository("repo");
        let linked = sandbox.root.join("linked");
        git(
            &repository,
            &["worktree", "add", "-b", "feature", linked.to_str().unwrap()],
        );
        let state_root = sandbox.root.join("state");

        let main =
            resolve_database_in(&repository, DatabaseKind::MindLeak, None, &state_root).unwrap();
        let worktree =
            resolve_database_in(&linked, DatabaseKind::MindLeak, None, &state_root).unwrap();

        assert_eq!(main.repository_id, worktree.repository_id);
        assert_eq!(main.path, worktree.path);
        assert_eq!(main.origin, DatabaseOrigin::Repository);
        assert_eq!(
            git(&linked, &["config", "--local", "--get", REPOSITORY_ID_KEY]),
            main.repository_id.unwrap()
        );
    }

    /// A local `git init` is already a repository. Requiring a commit or an
    /// `origin/main` ref would prevent both planes from starting precisely when
    /// their shared repository identity is needed for first-use bootstrap.
    #[test]
    fn an_unborn_repository_needs_neither_a_commit_nor_a_remote() {
        let sandbox = Sandbox::new("unborn");
        let repository = sandbox.root.join("repo");
        fs::create_dir_all(&repository).unwrap();
        git(&repository, &["init", "-b", "main"]);
        assert!(git(&repository, &["remote"]).is_empty());
        let state_root = sandbox.root.join("state");

        let mindleak =
            resolve_database_in(&repository, DatabaseKind::MindLeak, None, &state_root).unwrap();
        let lodestar =
            resolve_database_in(&repository, DatabaseKind::Lodestar, None, &state_root).unwrap();

        assert_eq!(mindleak.origin, DatabaseOrigin::Repository);
        assert_eq!(lodestar.origin, DatabaseOrigin::Repository);
        let repository_id = mindleak.repository_id.as_deref().unwrap();
        assert_eq!(lodestar.repository_id.as_deref(), Some(repository_id));
        let repository_state = state_root.join("repositories").join(repository_id);
        assert_eq!(mindleak.path, repository_state.join("graph.db"));
        assert_eq!(lodestar.path, repository_state.join("spec.db"));
    }

    #[test]
    fn independent_clones_receive_distinct_repository_ids() {
        let sandbox = Sandbox::new("clones");
        let source = sandbox.repository("source");
        let clone = sandbox.root.join("clone");
        git(
            &sandbox.root,
            &["clone", source.to_str().unwrap(), clone.to_str().unwrap()],
        );
        let state_root = sandbox.root.join("state");

        let source_state =
            resolve_database_in(&source, DatabaseKind::Lodestar, None, &state_root).unwrap();
        let clone_state =
            resolve_database_in(&clone, DatabaseKind::Lodestar, None, &state_root).unwrap();

        assert_ne!(source_state.repository_id, clone_state.repository_id);
        assert_ne!(source_state.path, clone_state.path);
    }

    #[test]
    fn simultaneous_first_starts_converge_on_one_id() {
        let sandbox = Sandbox::new("concurrent");
        let repository = sandbox.repository("repo");
        let state_root = sandbox.root.join("state");
        let mut workers = Vec::new();
        for _ in 0..8 {
            let repository = repository.clone();
            let state_root = state_root.clone();
            workers.push(thread::spawn(move || {
                resolve_database_in(&repository, DatabaseKind::MindLeak, None, &state_root)
                    .unwrap()
                    .repository_id
                    .unwrap()
            }));
        }
        let ids: Vec<String> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert!(ids.iter().all(|id| id == &ids[0]));
    }

    #[test]
    fn marker_reader_retries_content_being_written_by_the_winner() {
        let sandbox = Sandbox::new("partial-marker");
        let marker = sandbox.root.join("marker");
        fs::write(&marker, "partial").unwrap();
        let completed_marker = marker.clone();
        let writer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            fs::write(completed_marker, "0123456789abcdef0123456789abcdef\n").unwrap();
        });

        assert_eq!(
            read_repository_id_marker(&marker).unwrap(),
            "0123456789abcdef0123456789abcdef"
        );
        writer.join().unwrap();
    }

    #[test]
    fn explicit_database_wins_and_scratch_use_stays_workspace_local() {
        let sandbox = Sandbox::new("fallbacks");
        let scratch = sandbox.root.join("scratch");
        fs::create_dir_all(&scratch).unwrap();
        let state_root = sandbox.root.join("state");

        let explicit = resolve_database_in(
            &scratch,
            DatabaseKind::MindLeak,
            Some("custom/graph.db"),
            &state_root,
        )
        .unwrap();
        assert_eq!(explicit.path, PathBuf::from("custom/graph.db"));
        assert_eq!(explicit.origin, DatabaseOrigin::Explicit);

        let fallback =
            resolve_database_in(&scratch, DatabaseKind::Lodestar, None, &state_root).unwrap();
        assert_eq!(fallback.path, scratch.join(".lodestar").join("spec.db"));
        assert_eq!(fallback.origin, DatabaseOrigin::Workspace);
    }

    /// `resolve_database` (not the `_in` seam) still resolves an explicit path
    /// and a non-Git workspace correctly without ever needing a real platform
    /// state root, since both branches return before reaching it.
    #[test]
    fn resolve_database_honours_an_explicit_path_and_a_non_git_workspace() {
        let sandbox = Sandbox::new("public-resolve");
        let scratch = sandbox.root.join("scratch");
        fs::create_dir_all(&scratch).unwrap();

        let explicit =
            resolve_database(&scratch, DatabaseKind::MindLeak, Some("custom/graph.db")).unwrap();
        assert_eq!(explicit.path, PathBuf::from("custom/graph.db"));
        assert_eq!(explicit.origin, DatabaseOrigin::Explicit);
        assert_eq!(explicit.repository_id, None);
        assert_eq!(explicit.legacy_path, None);

        let fallback = resolve_database(&scratch, DatabaseKind::Lodestar, None).unwrap();
        assert_eq!(fallback.path, scratch.join(".lodestar").join("spec.db"));
        assert_eq!(fallback.origin, DatabaseOrigin::Workspace);
        assert_eq!(fallback.repository_id, None);
        assert_eq!(fallback.legacy_path, None);
    }

    /// Regression: a live session's own worktree can be reclaimed underneath it.
    /// The workspace path then names a directory that is simply gone, so `git
    /// rev-parse --git-common-dir` cannot even be spawned there and fails with
    /// `NotFound` -- the same error kind as "git is not installed". The resolver
    /// read that as "this was never a Git checkout" and silently handed back a
    /// fresh, empty, workspace-local database with no repository id. The symptom
    /// was `lodestar_stats` reporting 0 goals and 0 tasks where it had reported
    /// hundreds moments before, with no error and no warning anywhere. A
    /// directory that does not exist is never a legitimate workspace, so it is
    /// refused instead of fabricated.
    #[test]
    fn a_workspace_that_no_longer_exists_is_refused_rather_than_silently_emptied() {
        let sandbox = Sandbox::new("stale-workspace");
        let reclaimed = sandbox.root.join("reclaimed");
        fs::create_dir_all(&reclaimed).unwrap();
        let state_root = sandbox.root.join("state");

        // Present but outside Git: still a legitimate workspace-local fallback.
        let present =
            resolve_database_in(&reclaimed, DatabaseKind::Lodestar, None, &state_root).unwrap();
        assert_eq!(present.origin, DatabaseOrigin::Workspace);

        // Reclaimed out from under the still-running session.
        fs::remove_dir_all(&reclaimed).unwrap();

        let error = resolve_database_in(&reclaimed, DatabaseKind::Lodestar, None, &state_root)
            .expect_err("a vanished workspace must not resolve to a fresh empty database");
        assert!(
            matches!(&error, RepositoryStorageError::MissingWorkspace(path) if path == &reclaimed),
            "expected MissingWorkspace({}), got {error:?}",
            reclaimed.display()
        );

        // An explicit database path does not depend on the workspace, so it
        // still wins and must not be broken by the new refusal.
        let explicit = resolve_database_in(
            &reclaimed,
            DatabaseKind::Lodestar,
            Some("custom/spec.db"),
            &state_root,
        )
        .unwrap();
        assert_eq!(explicit.origin, DatabaseOrigin::Explicit);
    }

    /// Restores the prior `MINDLEAK_HOME` value on drop so this test cannot
    /// leak process-global state into any test that runs after it, panic or not.
    struct MindleakHomeGuard {
        previous: Option<String>,
    }

    impl MindleakHomeGuard {
        fn set(value: &Path) -> Self {
            let previous = std::env::var("MINDLEAK_HOME").ok();
            std::env::set_var("MINDLEAK_HOME", value);
            Self { previous }
        }
    }

    impl Drop for MindleakHomeGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var("MINDLEAK_HOME", value),
                None => std::env::remove_var("MINDLEAK_HOME"),
            }
        }
    }

    #[test]
    fn platform_state_root_honours_an_explicit_override() {
        let sandbox = Sandbox::new("state-root-override");
        let configured = sandbox.root.join("configured-home");
        let _guard = MindleakHomeGuard::set(&configured);

        assert_eq!(platform_state_root().unwrap(), configured);
    }

    #[test]
    fn workspace_hint_is_shared_and_relative_to_the_spawn_directory() {
        let current = Path::new("client");
        assert_eq!(resolve_workspace_path(current, None), current);
        assert_eq!(
            resolve_workspace_path(current, Some("worktrees/agent-a")),
            current.join("worktrees/agent-a")
        );
        let absolute = std::env::temp_dir().join("agent-b");
        assert_eq!(
            resolve_workspace_path(current, Some(absolute.to_str().unwrap())),
            absolute
        );
    }

    #[test]
    fn status_reports_the_resolved_startup_state_without_recomputation() {
        let resolved = ResolvedDatabase {
            path: PathBuf::from("state/repositories/abc/graph.db"),
            origin: DatabaseOrigin::Repository,
            repository_id: Some("0123456789abcdef0123456789abcdef".into()),
            legacy_path: Some(PathBuf::from("repo/.mindleak/graph.db")),
        };

        let status = resolved.status("mindleak", true);
        assert_eq!(status.plane, "mindleak");
        assert_eq!(status.database_path, resolved.path);
        assert_eq!(status.repository_id, resolved.repository_id);
        assert_eq!(status.legacy_path, resolved.legacy_path);
        assert_eq!(status.origin, DatabaseOrigin::Repository);
        assert!(status.migrated_legacy);
    }

    #[test]
    fn legacy_database_migrates_by_backup_without_deleting_source() {
        let sandbox = Sandbox::new("migration");
        let repository = sandbox.repository("repo");
        let state_root = sandbox.root.join("state");
        let resolved =
            resolve_database_in(&repository, DatabaseKind::MindLeak, None, &state_root).unwrap();
        let legacy = resolved.legacy_path.as_ref().unwrap();
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        let source = Connection::open(legacy).unwrap();
        source
            .execute_batch("CREATE TABLE facts(value TEXT); INSERT INTO facts VALUES ('kept');")
            .unwrap();
        drop(source);

        assert!(resolved.migrate_legacy_if_needed().unwrap());
        assert!(legacy.exists());
        assert!(!resolved.migrate_legacy_if_needed().unwrap());
        let migrated = Connection::open(&resolved.path).unwrap();
        let value: String = migrated
            .query_row("SELECT value FROM facts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "kept");
    }

    #[test]
    fn invalid_preconfigured_repository_id_is_rejected() {
        let sandbox = Sandbox::new("invalid-id");
        let repository = sandbox.repository("repo");
        git(
            &repository,
            &["config", "--local", REPOSITORY_ID_KEY, "not-an-id"],
        );

        let error = resolve_database_in(
            &repository,
            DatabaseKind::MindLeak,
            None,
            &sandbox.root.join("state"),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            RepositoryStorageError::InvalidRepositoryId(_)
        ));
    }

    /// The v0.1.3 release failed on this. `MINDLEAK_DB=":memory:"` resolves to a
    /// path with no directory component, and `Path::parent()` returns
    /// `Some("")` rather than `None`. `create_dir_all("")` short-circuits to
    /// `Ok`, so the bug hid behind the happy path — but the Unix branch then
    /// called `set_permissions("")`, which fails with `ENOENT`. The server
    /// exited immediately on Linux and macOS, reporting only
    /// "No such file or directory (os error 2)", while Windows started fine
    /// because it has no permissions call. Both release builds that actually
    /// executed a Unix binary failed; the macOS x64 job passed only because it
    /// is cross-compiled and skips execution.
    #[test]
    fn a_database_path_with_no_directory_needs_no_parent_created() {
        for path in [":memory:", "graph.db"] {
            let resolved = explicit_database_resolution(path);
            assert_eq!(
                resolved.path.parent().map(Path::to_path_buf),
                Some(PathBuf::new()),
                "{path} should expose an empty parent, which is the trap"
            );
            // Pin the trap itself, or this test passes on Windows for the wrong
            // reason and would not have caught the regression: the empty path is
            // only fatal because the Unix branch sets permissions on it.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert!(
                    fs::create_dir_all("").is_ok(),
                    "create_dir_all short-circuits"
                );
                let error = fs::set_permissions("", fs::Permissions::from_mode(0o700))
                    .expect_err("set_permissions on an empty path must fail");
                assert_eq!(error.kind(), io::ErrorKind::NotFound);
            }
            resolved
                .ensure_parent()
                .unwrap_or_else(|error| panic!("{path} must start cleanly, got {error}"));
        }
    }

    /// The fix must not stop real parents being created, or locked down.
    #[test]
    fn a_database_path_with_a_directory_still_creates_it() {
        let sandbox = Sandbox::new("ensure-parent");
        let nested = sandbox.root.join("deeper").join("still");
        let resolved = explicit_database_resolution(&nested.join("graph.db").to_string_lossy());

        resolved.ensure_parent().unwrap();

        assert!(nested.is_dir(), "the parent directory should exist");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&nested).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "the directory should stay private");
        }
    }

    #[test]
    fn migration_lock_acquires_and_writes_the_current_process_id() {
        let sandbox = Sandbox::new("migration-lock-acquire");
        let lock_path = sandbox.root.join("migration.lock");
        let destination = sandbox.root.join("graph.db");

        let lock = acquire_migration_lock(&lock_path, &destination)
            .unwrap()
            .expect("no concurrent holder, so the lock must be acquired");

        assert!(lock_path.exists());
        let recorded_pid: u32 = fs::read_to_string(&lock_path)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(recorded_pid, std::process::id());
        drop(lock);
    }

    #[test]
    fn migration_lock_returns_none_once_the_destination_already_exists() {
        let sandbox = Sandbox::new("migration-lock-done");
        let lock_path = sandbox.root.join("migration.lock");
        let destination = sandbox.root.join("graph.db");
        // A stranded lock from a holder that never cleaned up, but the
        // destination it was guarding already exists -- the migration finished.
        fs::write(&lock_path, "12345\n").unwrap();
        fs::write(&destination, "already migrated\n").unwrap();

        let lock = acquire_migration_lock(&lock_path, &destination).unwrap();

        assert!(
            lock.is_none(),
            "a finished migration must not hand back a lock"
        );
    }

    #[test]
    fn migration_lock_retries_until_a_concurrent_holder_releases_it() {
        let sandbox = Sandbox::new("migration-lock-retry");
        let lock_path = sandbox.root.join("migration.lock");
        let destination = sandbox.root.join("graph.db");
        fs::write(&lock_path, "99999\n").unwrap();

        let release_path = lock_path.clone();
        let releaser = thread::spawn(move || {
            thread::sleep(std::time::Duration::from_millis(30));
            fs::remove_file(&release_path).unwrap();
        });

        let lock = acquire_migration_lock(&lock_path, &destination)
            .unwrap()
            .expect("the lock must be acquired once the holder releases it");
        releaser.join().unwrap();

        assert!(lock_path.exists());
        let recorded_pid: u32 = fs::read_to_string(&lock_path)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(recorded_pid, std::process::id());
        drop(lock);
    }

    /// Regression for
    /// gaps.d/windows-migration-lock-retry-test-can-flake-with-permission-denied.md:
    /// a full workspace `cargo test --all` run flaked
    /// `migration_lock_retries_until_a_concurrent_holder_releases_it` with
    /// `PermissionDenied` instead of the release racing cleanly, while the
    /// isolated test passed every time -- the signature of a timing-sensitive
    /// window, not a deterministic assertion failure. On Windows, a concurrent
    /// `remove_file` does not clear the directory entry atomically the way
    /// Unix `unlink` does, so a racing `create_new` can observe
    /// `PermissionDenied` during the pending-delete window instead of the
    /// `AlreadyExists`/success the retry loop already handled. Single-shot
    /// repro is inherently timing-luck-dependent (the same reason the
    /// original flake needed a full workspace run under load to surface), so
    /// this repeats the exact race many times in a tight loop rather than
    /// once, the same trade-off the test above already makes for its own
    /// race. Fails without the `PermissionDenied` retry arm in
    /// `acquire_migration_lock`; passes with it.
    #[test]
    fn migration_lock_retry_survives_a_tight_release_race_repeatedly() {
        let sandbox = Sandbox::new("migration-lock-retry-stress");
        for i in 0..50 {
            let lock_path = sandbox.root.join(format!("migration-{i}.lock"));
            let destination = sandbox.root.join(format!("graph-{i}.db"));
            fs::write(&lock_path, "99999\n").unwrap();

            let release_path = lock_path.clone();
            let releaser = thread::spawn(move || {
                // No sleep: race the create_new attempt as tightly as possible
                // against the release, rather than the 30ms gap the single-shot
                // test above uses, to maximise the chance of landing inside the
                // Windows pending-delete window this test exists to catch.
                fs::remove_file(&release_path).unwrap();
            });

            let lock = acquire_migration_lock(&lock_path, &destination)
                .unwrap_or_else(|error| {
                    panic!("iteration {i}: retry loop must absorb the release race, not propagate it: {error}")
                })
                .expect("the lock must be acquired once the holder releases it");
            releaser.join().unwrap();
            drop(lock);
        }
    }

    #[test]
    fn migration_lock_drop_removes_the_lock_file() {
        let sandbox = Sandbox::new("migration-lock-drop");
        let lock_path = sandbox.root.join("migration.lock");
        let destination = sandbox.root.join("graph.db");

        let lock = acquire_migration_lock(&lock_path, &destination)
            .unwrap()
            .unwrap();
        assert!(lock_path.exists());

        drop(lock);

        assert!(!lock_path.exists());
    }
}
