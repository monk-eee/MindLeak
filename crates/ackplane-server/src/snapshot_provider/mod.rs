//! Executes and encrypts a platform-scoped Snapshot artifact (ADR-0119
//! decision 4, ADR-0128).
//!
//! This is the one place in `ackplane-server` that shells out to a
//! subprocess or touches a local filesystem path for administration
//! purposes; `administration_store` only ever records the immutable request
//! and receipt this module's outcome produces. Deliberately platform-scoped
//! only (see ADR-0128's session record): Ackplane's PostgreSQL schema is
//! multi-tenant at the row level, so a `pg_dump` of the whole database is
//! never a tenant-scoped artifact -- a true tenant-scoped export needs its
//! own per-table row-export implementation, tracked as separate follow-on
//! work, not this provider relabeled.

use std::{
    io,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant, SystemTime},
};

use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng},
    AeadCore, XChaCha20Poly1305, XNonce,
};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{fs, io::AsyncWriteExt, process::Command};
use tokio_postgres::NoTls;

/// Identifies which locally-generated key encrypted an artifact, without
/// ever carrying the key itself (ADR-0119 decision 4: key material never
/// enters a receipt, response, or log). Bumped only if the key file's format
/// or derivation ever changes; rotating the key file's *contents* does not
/// need a new id, because the id names the scheme, not the instance.
const ENCRYPTION_KEY_ID: &str = "ackplane-snapshot-key-v1";
const KEY_BYTES: usize = 32;
/// `XChaCha20Poly1305`'s extended nonce length -- the prefix
/// `create_platform_snapshot` writes ahead of the ciphertext.
const NONCE_BYTES: usize = 24;

/// ADR-0145 decision 3: how long a passing rehearsal of the exact artifact
/// digest stays trustworthy evidence for production execution. Chosen the
/// same order of magnitude as `MAX_CONFIRMATION_WINDOW`
/// (`purge_model.rs`) -- a rehearsal older than this proves nothing about
/// compatibility with whatever migrations landed since, so execution refuses
/// rather than trusting stale evidence.
pub const MAX_REHEARSAL_FRESHNESS: Duration = Duration::from_secs(24 * 3600);

/// Where a platform Snapshot is written and what encrypts it. Resolved once
/// from environment at Bridge/service startup, mirroring
/// `ackplane_bridge::BridgeConfig`'s own environment-resolved shape.
pub struct SnapshotProviderConfig {
    pub database_url: String,
    pub snapshot_dir: PathBuf,
    pub key_path: PathBuf,
    pub pg_dump_path: String,
    pub pg_restore_path: String,
    /// ADR-0145 decision 1: where a recovery rehearsal restores into --
    /// never `database_url`. `None` when `ACKPLANE_REHEARSAL_DATABASE_URL` is
    /// unset, in which case [`rehearse_recovery`] refuses outright rather
    /// than inventing a scratch target.
    pub rehearsal_database_url: Option<String>,
    /// ADR-0145 decision 6: whether an operator has attested that this
    /// deployment hosts exactly one tenant. Defaults to `false`, so production
    /// recovery execution is refused on every deployment until it is set
    /// explicitly.
    ///
    /// Deliberately never inferred from the number of tenant rows observed. A
    /// platform that happens to hold one tenant today can onboard a second
    /// tomorrow, and a restore that was safe when it was configured would
    /// silently stop being safe without anything changing in this file. This
    /// is a durable operator decision about the deployment's shape, not a
    /// runtime headcount.
    pub single_tenant_attested: bool,
}

impl SnapshotProviderConfig {
    pub const SNAPSHOT_DIR_ENV: &'static str = "ACKPLANE_SNAPSHOT_DIR";
    pub const KEY_PATH_ENV: &'static str = "ACKPLANE_SNAPSHOT_KEY_PATH";
    pub const PG_DUMP_PATH_ENV: &'static str = "ACKPLANE_PG_DUMP_PATH";
    pub const PG_RESTORE_PATH_ENV: &'static str = "ACKPLANE_PG_RESTORE_PATH";
    pub const REHEARSAL_DATABASE_URL_ENV: &'static str = "ACKPLANE_REHEARSAL_DATABASE_URL";
    pub const SINGLE_TENANT_ATTESTED_ENV: &'static str = "ACKPLANE_SINGLE_TENANT_ATTESTED";
    const DEFAULT_PG_DUMP_PATH: &'static str = "pg_dump";
    const DEFAULT_PG_RESTORE_PATH: &'static str = "pg_restore";

    /// `None` when `ACKPLANE_SNAPSHOT_DIR` is unset -- the Snapshot
    /// capability is then unavailable rather than falling back to a guessed
    /// location, the same "refuse, never invent a default" rule
    /// `BridgeConfig::resolve` already applies to its own required settings.
    pub fn resolve(lookup: impl Fn(&str) -> Option<String>, database_url: String) -> Option<Self> {
        let value = |key: &str| {
            lookup(key)
                .map(|raw| raw.trim().to_string())
                .filter(|raw| !raw.is_empty())
        };
        let snapshot_dir = PathBuf::from(value(Self::SNAPSHOT_DIR_ENV)?);
        let key_path = value(Self::KEY_PATH_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| snapshot_dir.join("snapshot-key.bin"));
        let pg_dump_path =
            value(Self::PG_DUMP_PATH_ENV).unwrap_or_else(|| Self::DEFAULT_PG_DUMP_PATH.to_string());
        let pg_restore_path = value(Self::PG_RESTORE_PATH_ENV)
            .unwrap_or_else(|| Self::DEFAULT_PG_RESTORE_PATH.to_string());
        let rehearsal_database_url = value(Self::REHEARSAL_DATABASE_URL_ENV);
        // Only an exact, unambiguous opt-in attests. Anything else -- unset,
        // "yes", "1", "TRUE ", a typo -- leaves it false, because the failure
        // direction matters: reading a malformed value as an attestation would
        // enable a whole-database restore on a platform nobody confirmed is
        // single-tenant, while reading it as absent only refuses a capability
        // the operator can re-enable by fixing the value.
        let single_tenant_attested = value(Self::SINGLE_TENANT_ATTESTED_ENV)
            .is_some_and(|raw| raw.eq_ignore_ascii_case("true"));
        Some(Self {
            database_url,
            snapshot_dir,
            key_path,
            pg_dump_path,
            pg_restore_path,
            rehearsal_database_url,
            single_tenant_attested,
        })
    }

    /// The gate production recovery execution must pass (ADR-0145 decision 6).
    ///
    /// Separate from executing anything, and checked before any destructive
    /// step, so the refusal is a decision about the deployment rather than a
    /// failure discovered partway through a restore. Slice 4's execution path
    /// calls this first; rehearsal deliberately does not, because a rehearsal
    /// restores into a scratch database and is useful on every deployment
    /// shape (decision 1).
    pub fn ensure_recovery_execution_permitted(&self) -> Result<(), SnapshotProviderError> {
        if self.single_tenant_attested {
            return Ok(());
        }
        Err(SnapshotProviderError::MultiTenantRecoveryUnavailable)
    }
}

/// What a successful Snapshot execution durably records (ADR-0119 decision
/// 4's receipt fields, minus what the caller already knows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotArtifact {
    pub artifact_path: String,
    pub manifest_digest: Vec<u8>,
    pub encryption_key_id: String,
    pub size_bytes: i64,
}

/// ADR-0119 decision 6: a read-only report against one identified Snapshot
/// artifact. Every check that failed is named in `reason`; none of them ever
/// touches the authoritative production database -- decryption happens
/// in-process and `pg_restore --list` only reads the decrypted archive file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectionReport {
    pub integrity_verified: bool,
    pub decryption_verified: bool,
    pub archive_valid: bool,
    pub archive_entry_count: Option<i64>,
    pub reason: String,
}

/// ADR-0145 decision 1-2: the outcome of one real restore drill against an
/// isolated, ephemeral target. Unlike [`InspectionReport`] (a format check
/// that never opens a database), this proves the archive actually restores
/// against this deployment's current schema -- migration compatibility and a
/// table-count reconciliation against the archive's own table of contents,
/// never a comparison against the authoritative production database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryRehearsalReport {
    pub manifest_digest: Vec<u8>,
    pub restore_duration_ms: i64,
    pub migration_version_matched: bool,
    /// `TABLE DATA` entries in the archive's own table of contents
    /// (`pg_restore --list`), established before any destructive action.
    /// `None` when the archive could not even be listed.
    pub archive_table_count: Option<i64>,
    /// Base tables actually present in the scratch database once restore and
    /// migration both completed. `None` when restore itself never reached
    /// that point.
    pub restored_table_count: Option<i64>,
    /// Total live row count across every restored table. Reported, not
    /// reconciled against an independently captured snapshot-time figure --
    /// `create_platform_snapshot` records no per-table row counts today, so
    /// there is nothing yet to compare this against beyond internal
    /// consistency (a restore that silently drops rows still restores every
    /// table, and the table-count reconciliation above would not catch it).
    pub restored_row_count: Option<i64>,
    pub passed: bool,
    pub reason: String,
}

/// ADR-0145 decision 7: the outcome of one production recovery-execution
/// restore -- real `pg_restore` against `config.database_url`, never a
/// scratch target. Unlike [`RecoveryRehearsalReport`], there is no migration
/// re-run or table reconciliation here: the freshness gate that admits a
/// caller to this function already proved schema compatibility via a recent
/// passing rehearsal of the identical artifact digest, and re-deriving that
/// proof here would duplicate rather than reuse it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryExecutionRestoreReport {
    pub restore_duration_ms: i64,
    pub succeeded: bool,
    pub reason: String,
}

#[derive(Debug, Error)]
pub enum SnapshotProviderError {
    #[error("could not prepare the snapshot directory or key: {0}")]
    Io(#[source] io::Error),
    #[error("could not start pg_dump ({path}): {source}")]
    Spawn { path: String, source: io::Error },
    #[error("pg_dump exited with a non-zero status: {stderr}")]
    PgDumpFailed { stderr: String },
    #[error("could not encrypt the snapshot artifact")]
    Encryption,
    #[error("could not start pg_restore ({path}): {source}")]
    RestoreSpawn { path: String, source: io::Error },
    #[error(
        "ACKPLANE_REHEARSAL_DATABASE_URL is not configured; rehearsal has nothing to restore into"
    )]
    RehearsalDatabaseNotConfigured,
    #[error("could not parse a rehearsal-relevant database url: {0}")]
    RehearsalDatabaseUrlInvalid(String),
    #[error(
        "the rehearsal database resolves to the same host/port/database as ACKPLANE_DATABASE_URL; \
         refusing to rehearse against the authoritative database"
    )]
    RehearsalDatabaseIsAuthoritative,
    #[error("could not provision the scratch rehearsal database: {0}")]
    RehearsalProvisionFailed(String),
    /// ADR-0145 decision 6. Every deployment hits this until an operator sets
    /// `ACKPLANE_SINGLE_TENANT_ATTESTED=true`, by design: a safe multi-tenant
    /// restore is a distinct, larger capability (restoring one tenant's rows
    /// out of a whole-database dump) that ADR-0145 does not design, so
    /// execution ships only for the shape where "restore everything" and
    /// "restore this tenant" are the same operation.
    #[error(
        "recovery execution is refused: this deployment is not attested single-tenant. \
         Restoring a platform Snapshot replaces the whole database, so on a multi-tenant \
         deployment it would overwrite every other tenant's data. Set \
         ACKPLANE_SINGLE_TENANT_ATTESTED=true only if this deployment hosts exactly one tenant"
    )]
    MultiTenantRecoveryUnavailable,
}

impl From<io::Error> for SnapshotProviderError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

mod create;
mod postgres;
mod recovery;

pub use create::{create_platform_snapshot, inspect_snapshot_artifact};
pub use recovery::{execute_recovery, rehearsal_is_fresh, rehearse_recovery};

// Reachable at `snapshot_provider::` because test_support already calls them there.
pub(crate) use postgres::{unique_suffix, with_dbname};

// Siblings cannot see each other, but every child can see these through the parent.
use create::{decrypt_sealed_artifact, DecryptedArtifact};
use postgres::{
    archive_table_data_count, filesystem_safe_id, reconcile_tables, run_pg_dump,
    split_authority_and_dbname, write_atomically,
};

use postgres::load_or_generate_key;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{rehearsal_test_url, with_ephemeral_database};

    fn temp_dir(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{prefix}-{}", std::process::id()))
    }

    #[test]
    fn resolve_returns_none_without_a_snapshot_dir() {
        let config = SnapshotProviderConfig::resolve(|_| None, "postgresql://x".to_string());
        assert!(config.is_none());
    }

    #[test]
    fn resolve_derives_a_default_key_path_beside_the_snapshot_dir() {
        let config = SnapshotProviderConfig::resolve(
            |key| {
                (key == SnapshotProviderConfig::SNAPSHOT_DIR_ENV).then(|| "/snapshots".to_string())
            },
            "postgresql://x".to_string(),
        )
        .expect("a snapshot dir alone should resolve");
        assert_eq!(
            config.key_path,
            PathBuf::from("/snapshots").join("snapshot-key.bin")
        );
        assert_eq!(config.pg_dump_path, "pg_dump");
    }

    /// Regression: the attestation must default to refusing.
    ///
    /// THE BUG THIS PREVENTS. Restoring a platform Snapshot replaces the whole
    /// database, so on a multi-tenant deployment it overwrites every other
    /// tenant's data. ADR-0145 decision 6 therefore ships execution only where
    /// an operator has explicitly attested the deployment hosts one tenant. A
    /// field defaulting to `true`, or inferred from anything, would enable the
    /// destructive path on every deployment that never thought about it.
    #[test]
    fn a_deployment_that_never_attested_refuses_recovery_execution() {
        let config = SnapshotProviderConfig::resolve(
            |key| {
                (key == SnapshotProviderConfig::SNAPSHOT_DIR_ENV).then(|| "/snapshots".to_string())
            },
            "postgresql://x".to_string(),
        )
        .expect("a snapshot dir alone should resolve");

        assert!(
            !config.single_tenant_attested,
            "an unset attestation must default to false"
        );
        assert!(matches!(
            config.ensure_recovery_execution_permitted(),
            Err(SnapshotProviderError::MultiTenantRecoveryUnavailable)
        ));
    }

    #[test]
    fn an_explicit_attestation_permits_recovery_execution() {
        let config = SnapshotProviderConfig::resolve(
            |key| match key {
                SnapshotProviderConfig::SNAPSHOT_DIR_ENV => Some("/snapshots".to_string()),
                SnapshotProviderConfig::SINGLE_TENANT_ATTESTED_ENV => Some("true".to_string()),
                _ => None,
            },
            "postgresql://x".to_string(),
        )
        .expect("a snapshot dir alone should resolve");

        assert!(config.single_tenant_attested);
        assert!(config.ensure_recovery_execution_permitted().is_ok());
    }

    /// Regression: only an exact opt-in attests.
    ///
    /// THE BUG THIS PREVENTS. The tempting implementation treats "is this
    /// variable set to something truthy?" generously — `"1"`, `"yes"`, `"on"`.
    /// Every one of those would let a typo, or a shell that exports `"0"` as a
    /// non-empty string, enable a whole-database restore on a platform nobody
    /// confirmed is single-tenant. The failure directions are not symmetric:
    /// reading a malformed value as absent only refuses a capability the
    /// operator can re-enable, while reading it as an attestation destroys
    /// other tenants' data.
    #[test]
    fn a_value_that_is_not_exactly_true_does_not_attest() {
        for raw in ["", " ", "1", "yes", "on", "false", "0", "trueish", "ture"] {
            let config = SnapshotProviderConfig::resolve(
                |key| match key {
                    SnapshotProviderConfig::SNAPSHOT_DIR_ENV => Some("/snapshots".to_string()),
                    SnapshotProviderConfig::SINGLE_TENANT_ATTESTED_ENV => Some(raw.to_string()),
                    _ => None,
                },
                "postgresql://x".to_string(),
            )
            .expect("a snapshot dir alone should resolve");

            assert!(
                !config.single_tenant_attested,
                "{raw:?} must not be read as an attestation"
            );
            assert!(config.ensure_recovery_execution_permitted().is_err());
        }
    }

    /// Case and surrounding whitespace are the operator's, not a second
    /// setting: `TRUE` and ` true ` are the same explicit answer.
    #[test]
    fn an_attestation_is_case_and_whitespace_insensitive() {
        for raw in ["true", "TRUE", "True", "  true  "] {
            let config = SnapshotProviderConfig::resolve(
                |key| match key {
                    SnapshotProviderConfig::SNAPSHOT_DIR_ENV => Some("/snapshots".to_string()),
                    SnapshotProviderConfig::SINGLE_TENANT_ATTESTED_ENV => Some(raw.to_string()),
                    _ => None,
                },
                "postgresql://x".to_string(),
            )
            .expect("a snapshot dir alone should resolve");

            assert!(config.single_tenant_attested, "{raw:?} must attest");
        }
    }

    /// The refusal has to tell an operator what to do about it. A typed error
    /// nobody can act on just moves the dead end.
    #[test]
    fn the_refusal_names_the_setting_that_would_permit_it() {
        let message = SnapshotProviderError::MultiTenantRecoveryUnavailable.to_string();
        assert!(
            message.contains("ACKPLANE_SINGLE_TENANT_ATTESTED"),
            "the refusal must name the setting, got {message}"
        );
        assert!(
            message.contains("exactly one tenant"),
            "the refusal must say when setting it is safe, got {message}"
        );
    }

    #[tokio::test]
    async fn load_or_generate_key_reuses_an_existing_key_on_later_calls() {
        let dir = temp_dir("ackplane-snapshot-key-test");
        let _ = fs::remove_dir_all(&dir).await;
        let path = dir.join("key.bin");

        let first = load_or_generate_key(&path)
            .await
            .expect("the first call should generate a key");
        let second = load_or_generate_key(&path)
            .await
            .expect("the second call should reuse the same key");
        assert_eq!(first, second);
        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn create_platform_snapshot_encrypts_and_records_a_verifiable_digest() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        if Command::new("pg_dump")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .is_err()
        {
            eprintln!("skipping: pg_dump is not available on PATH");
            return;
        }

        let dir = temp_dir("ackplane-snapshot-provider-test");
        let _ = fs::remove_dir_all(&dir).await;
        let config = SnapshotProviderConfig {
            database_url,
            snapshot_dir: dir.clone(),
            key_path: dir.join("snapshot-key.bin"),
            pg_dump_path: "pg_dump".to_string(),
            pg_restore_path: "pg_restore".to_string(),
            rehearsal_database_url: None,
            single_tenant_attested: false,
        };

        let artifact = create_platform_snapshot(&config, "test-request")
            .await
            .expect("a real pg_dump against the test database should succeed");
        assert_eq!(artifact.manifest_digest.len(), 32);
        assert!(artifact.size_bytes > 0);
        assert_eq!(artifact.encryption_key_id, ENCRYPTION_KEY_ID);

        let sealed = fs::read(&artifact.artifact_path)
            .await
            .expect("the encrypted artifact file should exist");
        assert_eq!(Sha256::digest(&sealed).to_vec(), artifact.manifest_digest);

        if Command::new("pg_restore")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .is_ok()
        {
            let report = inspect_snapshot_artifact(
                &config,
                &artifact.artifact_path,
                &artifact.manifest_digest,
            )
            .await
            .expect("inspecting a freshly created artifact should succeed");
            assert!(report.integrity_verified);
            assert!(report.decryption_verified);
            assert!(report.archive_valid, "reason was: {}", report.reason);
            assert!(report.archive_entry_count.unwrap_or_default() > 0);
        } else {
            eprintln!("skipping the inspection half: pg_restore is not available on PATH");
        }

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn inspecting_a_tampered_artifact_reports_a_digest_mismatch() {
        let dir = temp_dir("ackplane-snapshot-inspect-tamper-test");
        let _ = fs::remove_dir_all(&dir).await;
        fs::create_dir_all(&dir)
            .await
            .expect("creating the test directory should succeed");
        let artifact_path = dir.join("tampered.pgdump.enc");
        fs::write(
            &artifact_path,
            b"not the bytes the digest was computed over",
        )
        .await
        .expect("writing the tampered fixture should succeed");
        let config = SnapshotProviderConfig {
            database_url: "postgresql://unused".to_string(),
            snapshot_dir: dir.clone(),
            key_path: dir.join("snapshot-key.bin"),
            pg_dump_path: "pg_dump".to_string(),
            pg_restore_path: "pg_restore".to_string(),
            rehearsal_database_url: None,
            single_tenant_attested: false,
        };

        let report = inspect_snapshot_artifact(
            &config,
            artifact_path.to_str().expect("a valid UTF-8 path"),
            &[0_u8; 32],
        )
        .await
        .expect("inspection itself never errors; a mismatch is a reported finding");
        assert!(!report.integrity_verified);
        assert!(!report.decryption_verified);
        assert!(!report.archive_valid);

        let _ = fs::remove_dir_all(&dir).await;
    }

    fn base_rehearsal_config(
        dir: &Path,
        rehearsal_database_url: Option<String>,
    ) -> SnapshotProviderConfig {
        SnapshotProviderConfig {
            database_url: "postgresql://user:pass@localhost:5432/ackplane".to_string(),
            snapshot_dir: dir.to_path_buf(),
            key_path: dir.join("snapshot-key.bin"),
            pg_dump_path: "pg_dump".to_string(),
            pg_restore_path: "pg_restore".to_string(),
            rehearsal_database_url,
            single_tenant_attested: false,
        }
    }

    #[tokio::test]
    async fn rehearsal_refuses_when_unconfigured() {
        let dir = temp_dir("ackplane-rehearsal-unconfigured-test");
        let config = base_rehearsal_config(&dir, None);
        let error = rehearse_recovery(&config, "/does/not/matter", &[0_u8; 32])
            .await
            .expect_err("rehearsal with no configured target must be refused");
        assert!(matches!(
            error,
            SnapshotProviderError::RehearsalDatabaseNotConfigured
        ));
    }

    #[tokio::test]
    async fn rehearsal_refuses_when_the_rehearsal_url_names_the_authoritative_database() {
        let dir = temp_dir("ackplane-rehearsal-same-db-test");
        let mut config = base_rehearsal_config(&dir, None);
        config.rehearsal_database_url = Some(config.database_url.clone());
        let error = rehearse_recovery(&config, "/does/not/matter", &[0_u8; 32])
            .await
            .expect_err("a rehearsal url naming the authoritative database must be refused");
        assert!(matches!(
            error,
            SnapshotProviderError::RehearsalDatabaseIsAuthoritative
        ));
    }

    #[tokio::test]
    async fn a_freshly_migrated_source_rehearses_and_passes() {
        let Some(rehearsal_url) = rehearsal_test_url() else {
            eprintln!("skipping: ACKPLANE_TEST_REHEARSAL_DATABASE_URL is not set");
            return;
        };
        if Command::new("pg_dump")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .is_err()
            || Command::new("pg_restore")
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .is_err()
        {
            eprintln!("skipping: pg_dump/pg_restore is not available on PATH");
            return;
        }

        let rehearsal_url_for_config = rehearsal_url.clone();
        with_ephemeral_database(
            &rehearsal_url,
            "ackplane_rehearsal_pass_source",
            |source_url| async move {
                let source_pool =
                    crate::db_pool::build_pool(&source_url, crate::db_pool::TEST_POOL_MAX_SIZE)
                        .expect("the ephemeral source database url should build a pool");
                crate::ledger::LedgerStore::connect(&source_pool)
                    .await
                    .expect("migrating the ephemeral source database should succeed");

                let dir = temp_dir("ackplane-rehearsal-pass-test");
                let _ = fs::remove_dir_all(&dir).await;
                let mut config = base_rehearsal_config(&dir, Some(rehearsal_url_for_config));
                config.database_url = source_url;

                let artifact = create_platform_snapshot(&config, "rehearsal-pass-test")
                    .await
                    .expect("a real pg_dump against the fresh source database should succeed");

                let report =
                    rehearse_recovery(&config, &artifact.artifact_path, &artifact.manifest_digest)
                        .await
                        .expect("rehearsal itself should not error for a genuine artifact");

                assert!(
                    report.migration_version_matched,
                    "reason was: {}",
                    report.reason
                );
                assert!(report.passed, "reason was: {}", report.reason);
                assert!(report.restored_table_count.unwrap_or_default() > 0);
                assert!(
                    report.restored_table_count.unwrap_or_default()
                        >= report.archive_table_count.unwrap_or_default()
                );

                let _ = fs::remove_dir_all(&dir).await;
            },
        )
        .await;
    }

    #[tokio::test]
    async fn a_tampered_migration_digest_is_reported_as_a_mismatch() {
        let Some(rehearsal_url) = rehearsal_test_url() else {
            eprintln!("skipping: ACKPLANE_TEST_REHEARSAL_DATABASE_URL is not set");
            return;
        };
        if Command::new("pg_dump")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .is_err()
            || Command::new("pg_restore")
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .is_err()
        {
            eprintln!("skipping: pg_dump/pg_restore is not available on PATH");
            return;
        }

        let rehearsal_url_for_config = rehearsal_url.clone();
        with_ephemeral_database(
            &rehearsal_url,
            "ackplane_rehearsal_mismatch_source",
            |source_url| async move {
                let source_pool =
                    crate::db_pool::build_pool(&source_url, crate::db_pool::TEST_POOL_MAX_SIZE)
                        .expect("the ephemeral source database url should build a pool");
                crate::ledger::LedgerStore::connect(&source_pool)
                    .await
                    .expect("migrating the ephemeral source database should succeed");

                // Tamper the recorded digest for the ledger's migration key
                // so it no longer matches what this binary's migration file
                // actually contains -- simulating a restored database whose
                // recorded schema predates an incompatible change. Isolated
                // to this ephemeral source database, never the shared
                // ACKPLANE_TEST_DATABASE_URL.
                let (client, connection) = tokio_postgres::connect(&source_url, NoTls)
                    .await
                    .expect("a direct fixture connection should succeed");
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                client
                    .execute(
                        "UPDATE ackplane_schema_migrations SET content_digest = \
                         'deliberately-wrong-digest' WHERE migration_key = 1",
                        &[],
                    )
                    .await
                    .expect("tampering the recorded digest should succeed");

                let dir = temp_dir("ackplane-rehearsal-mismatch-test");
                let _ = fs::remove_dir_all(&dir).await;
                let mut config = base_rehearsal_config(&dir, Some(rehearsal_url_for_config));
                config.database_url = source_url;

                let artifact = create_platform_snapshot(&config, "rehearsal-mismatch-test")
                    .await
                    .expect("a real pg_dump against the tampered source database should succeed");

                let report =
                    rehearse_recovery(&config, &artifact.artifact_path, &artifact.manifest_digest)
                        .await
                        .expect("rehearsal itself should not error for a genuine artifact");

                assert!(
                    !report.migration_version_matched,
                    "expected a migration mismatch, reason was: {}",
                    report.reason
                );
                assert!(!report.passed);

                let _ = fs::remove_dir_all(&dir).await;
            },
        )
        .await;
    }

    #[test]
    fn rehearsal_is_fresh_accepts_within_the_window_and_refuses_past_it() {
        let now = SystemTime::now();
        assert!(rehearsal_is_fresh(now, now));
        assert!(rehearsal_is_fresh(now - Duration::from_secs(3600), now));
        assert!(rehearsal_is_fresh(now - MAX_REHEARSAL_FRESHNESS, now));
        assert!(!rehearsal_is_fresh(
            now - MAX_REHEARSAL_FRESHNESS - Duration::from_secs(1),
            now
        ));
    }

    /// Regression: a rehearsal timestamped in the future (clock skew, or a
    /// corrupted record) must not read as infinitely fresh.
    #[test]
    fn rehearsal_is_fresh_refuses_a_rehearsal_timestamped_in_the_future() {
        let now = SystemTime::now();
        assert!(!rehearsal_is_fresh(now + Duration::from_secs(3600), now));
    }

    #[tokio::test]
    async fn execute_recovery_refuses_outright_when_not_single_tenant_attested() {
        let dir = temp_dir("ackplane-execute-recovery-unattested-test");
        let config = base_rehearsal_config(&dir, None);
        assert!(!config.single_tenant_attested);
        let error = execute_recovery(&config, "/does/not/matter", &[0_u8; 32])
            .await
            .expect_err("execution must refuse outright on an unattested deployment");
        assert!(matches!(
            error,
            SnapshotProviderError::MultiTenantRecoveryUnavailable
        ));
    }

    #[tokio::test]
    async fn execute_recovery_reports_a_digest_mismatch_without_attempting_pg_restore() {
        let dir = temp_dir("ackplane-execute-recovery-digest-mismatch-test");
        fs::create_dir_all(&dir)
            .await
            .expect("creating the fixture directory should succeed");
        let artifact_path = dir.join("artifact.pgdump.enc");
        fs::write(&artifact_path, b"not a real sealed artifact")
            .await
            .expect("writing the fixture artifact should succeed");
        let mut config = base_rehearsal_config(&dir, None);
        config.single_tenant_attested = true;
        // A real, readable file whose content does not hash to the expected
        // digest -- the digest check must fail (and be reported, not raised
        // as an error) before anything tries to decrypt or restore it.
        let report = execute_recovery(&config, &artifact_path.to_string_lossy(), &[0_u8; 32])
            .await
            .expect("a digest mismatch is a reported failure, not an error");
        assert!(!report.succeeded, "reason was: {}", report.reason);
        assert_eq!(report.restore_duration_ms, 0);
        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn a_real_restore_against_an_ephemeral_target_succeeds() {
        let Some(rehearsal_url) = rehearsal_test_url() else {
            eprintln!("skipping: ACKPLANE_TEST_REHEARSAL_DATABASE_URL is not set");
            return;
        };
        if Command::new("pg_dump")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .is_err()
            || Command::new("pg_restore")
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .is_err()
        {
            eprintln!("skipping: pg_dump/pg_restore is not available on PATH");
            return;
        }

        let rehearsal_url_for_source = rehearsal_url.clone();
        with_ephemeral_database(
            &rehearsal_url,
            "ackplane_execute_recovery_source",
            |source_url| async move {
                let source_pool =
                    crate::db_pool::build_pool(&source_url, crate::db_pool::TEST_POOL_MAX_SIZE)
                        .expect("the ephemeral source database url should build a pool");
                crate::ledger::LedgerStore::connect(&source_pool)
                    .await
                    .expect("migrating the ephemeral source database should succeed");

                let dir = temp_dir("ackplane-execute-recovery-success-test");
                let _ = fs::remove_dir_all(&dir).await;
                let mut config =
                    base_rehearsal_config(&dir, Some(rehearsal_url_for_source.clone()));
                config.database_url = source_url;

                let artifact = create_platform_snapshot(&config, "execute-recovery-success-test")
                    .await
                    .expect("a real pg_dump against the source database should succeed");

                // A second, independently migrated ephemeral database stands
                // in for "production" -- already-current schema and already
                // holding objects, exactly what `--clean --if-exists` must
                // tolerate that a fresh, empty scratch database (rehearsal's
                // own target) never exercises.
                with_ephemeral_database(
                    &rehearsal_url_for_source,
                    "ackplane_execute_recovery_target",
                    |target_url| async move {
                        let target_pool = crate::db_pool::build_pool(
                            &target_url,
                            crate::db_pool::TEST_POOL_MAX_SIZE,
                        )
                        .expect("the ephemeral target database url should build a pool");
                        crate::ledger::LedgerStore::connect(&target_pool)
                            .await
                            .expect("migrating the ephemeral target database should succeed");

                        let mut execution_config = config;
                        execution_config.database_url = target_url;
                        execution_config.single_tenant_attested = true;

                        let report = execute_recovery(
                            &execution_config,
                            &artifact.artifact_path,
                            &artifact.manifest_digest,
                        )
                        .await
                        .expect("a genuine artifact must not error");
                        assert!(report.succeeded, "reason was: {}", report.reason);
                        assert!(report.restore_duration_ms >= 0);
                    },
                )
                .await;

                let _ = fs::remove_dir_all(&dir).await;
            },
        )
        .await;
    }
}
