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
    time::Instant,
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

/// Runs `pg_dump` against the deployment's own `ACKPLANE_DATABASE_URL`,
/// encrypts the result with a locally-generated key (never the operator's
/// database credentials), and writes it under `snapshot_dir`. Platform scope
/// only: see this module's own doc comment for why a tenant-scoped snapshot
/// is a distinct, unimplemented capability.
pub async fn create_platform_snapshot(
    config: &SnapshotProviderConfig,
    request_id: &str,
) -> Result<SnapshotArtifact, SnapshotProviderError> {
    fs::create_dir_all(&config.snapshot_dir).await?;
    let key = load_or_generate_key(&config.key_path).await?;
    let cipher = XChaCha20Poly1305::new((&key).into());

    let dump = run_pg_dump(&config.pg_dump_path, &config.database_url).await?;
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, dump.as_slice())
        .map_err(|_| SnapshotProviderError::Encryption)?;

    // The nonce travels with the artifact (it is not secret; only the key
    // is), prefixed so a restore knows exactly which bytes to split.
    let mut sealed = Vec::with_capacity(nonce.len() + ciphertext.len());
    sealed.extend_from_slice(&nonce);
    sealed.extend_from_slice(&ciphertext);

    let manifest_digest = Sha256::digest(&sealed).to_vec();
    let artifact_path = config
        .snapshot_dir
        .join(format!("{}.pgdump.enc", filesystem_safe_id(request_id)));
    write_atomically(&artifact_path, &sealed).await?;

    Ok(SnapshotArtifact {
        artifact_path: artifact_path.to_string_lossy().into_owned(),
        manifest_digest,
        encryption_key_id: ENCRYPTION_KEY_ID.to_string(),
        size_bytes: i64::try_from(sealed.len()).unwrap_or(i64::MAX),
    })
}

/// Reads, digest-checks, and decrypts a sealed artifact -- shared by
/// [`inspect_snapshot_artifact`] and [`rehearse_recovery`], which both need
/// the identical integrity-then-decrypt steps before doing anything with the
/// plaintext archive.
enum DecryptedArtifact {
    DigestMismatch,
    TooShort,
    DecryptionFailed,
    Ready(Vec<u8>),
}

async fn decrypt_sealed_artifact(
    config: &SnapshotProviderConfig,
    artifact_path: &str,
    expected_digest: &[u8],
) -> Result<DecryptedArtifact, SnapshotProviderError> {
    let sealed = fs::read(artifact_path).await?;
    let actual_digest = Sha256::digest(&sealed).to_vec();
    if actual_digest != expected_digest {
        return Ok(DecryptedArtifact::DigestMismatch);
    }
    if sealed.len() <= NONCE_BYTES {
        return Ok(DecryptedArtifact::TooShort);
    }

    let (nonce_bytes, ciphertext) = sealed.split_at(NONCE_BYTES);
    let key = load_or_generate_key(&config.key_path).await?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    match cipher.decrypt(XNonce::from_slice(nonce_bytes), ciphertext) {
        Ok(plaintext) => Ok(DecryptedArtifact::Ready(plaintext)),
        Err(_) => Ok(DecryptedArtifact::DecryptionFailed),
    }
}

/// ADR-0119 decision 6: inspects one identified Snapshot artifact -- digest
/// integrity, decryptability with the installation's own key, and archive
/// validity via `pg_restore --list` -- without ever touching the
/// authoritative production database. Every failure is a reported finding,
/// not an error: a tampered, undecryptable, or corrupt artifact is exactly
/// what this exists to detect.
pub async fn inspect_snapshot_artifact(
    config: &SnapshotProviderConfig,
    artifact_path: &str,
    expected_digest: &[u8],
) -> Result<InspectionReport, SnapshotProviderError> {
    let plaintext = match decrypt_sealed_artifact(config, artifact_path, expected_digest).await? {
        DecryptedArtifact::DigestMismatch => {
            return Ok(InspectionReport {
                integrity_verified: false,
                decryption_verified: false,
                archive_valid: false,
                archive_entry_count: None,
                reason: "The artifact's digest no longer matches its recorded manifest digest."
                    .to_string(),
            })
        }
        DecryptedArtifact::TooShort => {
            return Ok(InspectionReport {
                integrity_verified: true,
                decryption_verified: false,
                archive_valid: false,
                archive_entry_count: None,
                reason: "The artifact is too short to contain a nonce and any ciphertext."
                    .to_string(),
            })
        }
        DecryptedArtifact::DecryptionFailed => {
            return Ok(InspectionReport {
                integrity_verified: true,
                decryption_verified: false,
                archive_valid: false,
                archive_entry_count: None,
                reason:
                    "The artifact could not be decrypted with this installation's snapshot key."
                        .to_string(),
            })
        }
        DecryptedArtifact::Ready(plaintext) => plaintext,
    };

    let temp_path = std::env::temp_dir().join(format!(
        "ackplane-snapshot-inspect-{}-{}.pgdump",
        std::process::id(),
        unique_suffix()
    ));
    fs::write(&temp_path, &plaintext).await?;
    let output = Command::new(&config.pg_restore_path)
        .arg("--list")
        .arg(&temp_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await;
    let _ = fs::remove_file(&temp_path).await;
    let output = output.map_err(|source| SnapshotProviderError::RestoreSpawn {
        path: config.pg_restore_path.clone(),
        source,
    })?;

    if output.status.success() {
        let entry_count = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| {
                let line = line.trim();
                !line.is_empty() && !line.starts_with(';')
            })
            .count();
        Ok(InspectionReport {
            integrity_verified: true,
            decryption_verified: true,
            archive_valid: true,
            archive_entry_count: Some(i64::try_from(entry_count).unwrap_or(i64::MAX)),
            reason: format!("pg_restore --list reported {entry_count} archive entries."),
        })
    } else {
        Ok(InspectionReport {
            integrity_verified: true,
            decryption_verified: true,
            archive_valid: false,
            archive_entry_count: None,
            reason: format!(
                "pg_restore --list failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        })
    }
}

/// ADR-0145 decision 1: a real restore drill against an isolated, ephemeral
/// scratch database -- never `config.database_url`, the authoritative
/// production database. Proves the archive actually restores against this
/// deployment's current schema (`crate::schema_migration`'s exact migration
/// sequence, the same one `bin/migrate.rs` runs at boot, not a parallel copy
/// of it) and reconciles the restored table count against the archive's own
/// table of contents.
///
/// Requires `config.rehearsal_database_url`
/// (`ACKPLANE_REHEARSAL_DATABASE_URL`) to be configured, and refuses outright
/// -- a fatal misconfiguration, not a warning -- if it names the same
/// host/port/database as `config.database_url`: rehearsing against
/// production would defeat the entire point of an isolated target. That
/// comparison is a best-effort parse of both URIs' authority and `dbname`
/// path segment (documented limitation: a `key=value` DSN, or two distinct
/// host strings that happen to resolve to the same server, are not
/// detected); when either url cannot be parsed this way, rehearsal refuses
/// with [`SnapshotProviderError::RehearsalDatabaseUrlInvalid`] rather than
/// guessing.
pub async fn rehearse_recovery(
    config: &SnapshotProviderConfig,
    artifact_path: &str,
    expected_digest: &[u8],
) -> Result<RecoveryRehearsalReport, SnapshotProviderError> {
    let rehearsal_url = config
        .rehearsal_database_url
        .as_deref()
        .ok_or(SnapshotProviderError::RehearsalDatabaseNotConfigured)?;

    let authoritative = split_authority_and_dbname(&config.database_url).ok_or_else(|| {
        SnapshotProviderError::RehearsalDatabaseUrlInvalid(
            "ACKPLANE_DATABASE_URL is not a postgresql:// uri".to_string(),
        )
    })?;
    let rehearsal = split_authority_and_dbname(rehearsal_url).ok_or_else(|| {
        SnapshotProviderError::RehearsalDatabaseUrlInvalid(
            "ACKPLANE_REHEARSAL_DATABASE_URL is not a postgresql:// uri".to_string(),
        )
    })?;
    if authoritative == rehearsal {
        return Err(SnapshotProviderError::RehearsalDatabaseIsAuthoritative);
    }

    let plaintext = match decrypt_sealed_artifact(config, artifact_path, expected_digest).await? {
        DecryptedArtifact::DigestMismatch => {
            return Ok(RecoveryRehearsalReport {
                manifest_digest: expected_digest.to_vec(),
                restore_duration_ms: 0,
                migration_version_matched: false,
                archive_table_count: None,
                restored_table_count: None,
                restored_row_count: None,
                passed: false,
                reason: "The artifact's digest no longer matches its recorded manifest digest."
                    .to_string(),
            })
        }
        DecryptedArtifact::TooShort => {
            return Ok(RecoveryRehearsalReport {
                manifest_digest: expected_digest.to_vec(),
                restore_duration_ms: 0,
                migration_version_matched: false,
                archive_table_count: None,
                restored_table_count: None,
                restored_row_count: None,
                passed: false,
                reason: "The artifact is too short to contain a nonce and any ciphertext."
                    .to_string(),
            })
        }
        DecryptedArtifact::DecryptionFailed => {
            return Ok(RecoveryRehearsalReport {
                manifest_digest: expected_digest.to_vec(),
                restore_duration_ms: 0,
                migration_version_matched: false,
                archive_table_count: None,
                restored_table_count: None,
                restored_row_count: None,
                passed: false,
                reason:
                    "The artifact could not be decrypted with this installation's snapshot key."
                        .to_string(),
            })
        }
        DecryptedArtifact::Ready(plaintext) => plaintext,
    };

    let temp_path = std::env::temp_dir().join(format!(
        "ackplane-rehearsal-restore-{}-{}.pgdump",
        std::process::id(),
        unique_suffix()
    ));
    fs::write(&temp_path, &plaintext).await?;

    // The archive's own manifest: how many tables it carries data for,
    // established via `pg_restore --list` before any destructive action --
    // read-only against the archive file, never a database.
    let archive_table_count = archive_table_data_count(&config.pg_restore_path, &temp_path).await;

    let scratch_name = format!("ackplane_rehearsal_{}", unique_suffix());
    let result = run_rehearsal(
        config,
        rehearsal_url,
        &scratch_name,
        &temp_path,
        archive_table_count,
        expected_digest,
    )
    .await;

    let _ = fs::remove_file(&temp_path).await;
    result
}

/// Provisions the scratch database, runs the rehearsal against it, and drops
/// it again -- cleanup happens even when the rehearsal itself failed, since a
/// failed rehearsal that also leaks a scratch database compounds the problem
/// rather than just reporting it.
async fn run_rehearsal(
    config: &SnapshotProviderConfig,
    rehearsal_url: &str,
    scratch_name: &str,
    temp_path: &Path,
    archive_table_count: Option<i64>,
    expected_digest: &[u8],
) -> Result<RecoveryRehearsalReport, SnapshotProviderError> {
    let (maintenance_client, maintenance_connection) =
        tokio_postgres::connect(rehearsal_url, NoTls)
            .await
            .map_err(|error| {
                SnapshotProviderError::RehearsalProvisionFailed(format!(
                    "could not connect to the rehearsal maintenance database: {error}"
                ))
            })?;
    tokio::spawn(async move {
        let _ = maintenance_connection.await;
    });
    maintenance_client
        .batch_execute(&format!("CREATE DATABASE \"{scratch_name}\""))
        .await
        .map_err(|error| {
            SnapshotProviderError::RehearsalProvisionFailed(format!(
                "could not create the scratch rehearsal database: {error}"
            ))
        })?;

    let scratch_url = with_dbname(rehearsal_url, scratch_name).ok_or_else(|| {
        SnapshotProviderError::RehearsalDatabaseUrlInvalid(
            "could not build a scratch-database url from ACKPLANE_REHEARSAL_DATABASE_URL"
                .to_string(),
        )
    })?;

    let result = rehearse_against_scratch(
        config,
        &scratch_url,
        temp_path,
        archive_table_count,
        expected_digest,
    )
    .await;

    // Best-effort cleanup: a rehearsal that leaves a stray scratch database
    // behind is still meaningful evidence, so a cleanup failure is logged,
    // not folded into `passed`.
    if let Err(error) = maintenance_client
        .batch_execute(&format!("DROP DATABASE IF EXISTS \"{scratch_name}\""))
        .await
    {
        tracing::warn!(
            %error,
            scratch_name,
            "ackplane recovery rehearsal could not drop its scratch database"
        );
    }

    result
}

/// Restores the archive into the already-provisioned scratch database, runs
/// this deployment's exact migration sequence against it, and reconciles the
/// restored table count against the archive's own manifest.
async fn rehearse_against_scratch(
    config: &SnapshotProviderConfig,
    scratch_url: &str,
    temp_path: &Path,
    archive_table_count: Option<i64>,
    expected_digest: &[u8],
) -> Result<RecoveryRehearsalReport, SnapshotProviderError> {
    let restore_started = Instant::now();
    let output = Command::new(&config.pg_restore_path)
        .arg("--no-owner")
        .arg("-d")
        .arg(scratch_url)
        .arg(temp_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|source| SnapshotProviderError::RestoreSpawn {
            path: config.pg_restore_path.clone(),
            source,
        })?;
    let restore_duration_ms =
        i64::try_from(restore_started.elapsed().as_millis()).unwrap_or(i64::MAX);

    if !output.status.success() {
        return Ok(RecoveryRehearsalReport {
            manifest_digest: expected_digest.to_vec(),
            restore_duration_ms,
            migration_version_matched: false,
            archive_table_count,
            restored_table_count: None,
            restored_row_count: None,
            passed: false,
            reason: format!(
                "pg_restore into the scratch database failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }

    let migration_version_matched = match crate::schema_migration::migrate_all(scratch_url).await {
        Ok(()) => true,
        Err(error) => {
            return Ok(RecoveryRehearsalReport {
                manifest_digest: expected_digest.to_vec(),
                restore_duration_ms,
                migration_version_matched: false,
                archive_table_count,
                restored_table_count: None,
                restored_row_count: None,
                passed: false,
                reason: format!(
                    "the restored schema does not match this deployment's current migrations: \
                     {error}"
                ),
            })
        }
    };

    let (restored_table_count, restored_row_count) = match reconcile_tables(scratch_url).await {
        Ok(counts) => counts,
        Err(error) => {
            return Ok(RecoveryRehearsalReport {
                manifest_digest: expected_digest.to_vec(),
                restore_duration_ms,
                migration_version_matched,
                archive_table_count,
                restored_table_count: None,
                restored_row_count: None,
                passed: false,
                reason: format!("could not reconcile the restored table/row counts: {error}"),
            })
        }
    };

    let table_count_reconciled = match (archive_table_count, restored_table_count) {
        (Some(archive), Some(restored)) => restored >= archive,
        _ => false,
    };
    let passed = migration_version_matched && table_count_reconciled;
    let reason = if passed {
        format!(
            "restored and migrated in {restore_duration_ms}ms: {restored_table_count:?} tables \
             ({archive_table_count:?} expected from the archive), {restored_row_count:?} total rows."
        )
    } else if !migration_version_matched {
        "the restored schema does not match this deployment's current migrations".to_string()
    } else {
        format!(
            "restored table count {restored_table_count:?} does not cover the archive's own \
             {archive_table_count:?} table(s)"
        )
    };

    Ok(RecoveryRehearsalReport {
        manifest_digest: expected_digest.to_vec(),
        restore_duration_ms,
        migration_version_matched,
        archive_table_count,
        restored_table_count,
        restored_row_count,
        passed,
        reason,
    })
}

/// Counts `TABLE DATA` entries in the archive's own table of contents,
/// read-only against the decrypted archive file via `pg_restore --list`,
/// mirroring `inspect_snapshot_artifact`'s existing invocation. `None` when
/// `pg_restore --list` itself fails to run or reports a non-zero status,
/// since the reconciliation this feeds has nothing to reconcile against
/// without it.
async fn archive_table_data_count(pg_restore_path: &str, temp_path: &Path) -> Option<i64> {
    let output = Command::new(pg_restore_path)
        .arg("--list")
        .arg(temp_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let count = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains(" TABLE DATA "))
        .count();
    Some(i64::try_from(count).unwrap_or(i64::MAX))
}

/// Connects to the scratch database and reports the base table count
/// (excluding system schemas) and the total live row count summed across
/// every one of them.
async fn reconcile_tables(scratch_url: &str) -> Result<(Option<i64>, Option<i64>), String> {
    let (client, connection) = tokio_postgres::connect(scratch_url, NoTls)
        .await
        .map_err(|error| format!("could not connect to the scratch database: {error}"))?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let rows = client
        .query(
            "SELECT table_schema, table_name FROM information_schema.tables \
             WHERE table_schema NOT IN ('pg_catalog', 'information_schema') \
               AND table_type = 'BASE TABLE'",
            &[],
        )
        .await
        .map_err(|error| format!("could not list restored tables: {error}"))?;

    let table_count = i64::try_from(rows.len()).unwrap_or(i64::MAX);
    let mut total_rows: i64 = 0;
    for row in &rows {
        let schema: String = row.get(0);
        let table: String = row.get(1);
        let count_row = client
            .query_one(
                &format!(
                    "SELECT COUNT(*) FROM {}.{}",
                    quote_ident(&schema),
                    quote_ident(&table)
                ),
                &[],
            )
            .await
            .map_err(|error| format!("could not count rows in {schema}.{table}: {error}"))?;
        let rows_in_table: i64 = count_row.get(0);
        total_rows = total_rows.saturating_add(rows_in_table);
    }

    Ok((Some(table_count), Some(total_rows)))
}

/// Double-quotes a Postgres identifier for interpolation into a query,
/// doubling any internal `"` -- `information_schema` names are trusted
/// metadata, not caller input, but this is one line of defence either way.
fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// Splits a `postgresql://[user[:pass]@]host[:port]/dbname[?params]` url into
/// its authority (`user:pass@host:port`) and `dbname` path segment, so the
/// same-database refusal and scratch-url construction below never have to
/// reparse it differently. `None` for a `key=value` connection string, or any
/// url missing a `://` or a path segment.
fn split_authority_and_dbname(url: &str) -> Option<(&str, &str)> {
    let base = url.split('?').next().unwrap_or(url);
    let scheme_end = base.find("://")? + 3;
    let rest = base.get(scheme_end..)?;
    let path_start = rest.find('/')?;
    Some((&rest[..path_start], &rest[path_start + 1..]))
}

/// Rebuilds `url` with its `dbname` path segment replaced by `new_dbname`,
/// preserving the authority and any query string. `None` under the same
/// conditions as [`split_authority_and_dbname`].
fn with_dbname(url: &str, new_dbname: &str) -> Option<String> {
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };
    let scheme_end = base.find("://")? + 3;
    let rest = base.get(scheme_end..)?;
    let path_start = rest.find('/')?;
    let mut result = format!("{}/{new_dbname}", &base[..scheme_end + path_start]);
    if let Some(query) = query {
        result.push('?');
        result.push_str(query);
    }
    Some(result)
}

fn unique_suffix() -> String {
    let mut bytes = [0_u8; 8];
    let _ = getrandom::getrandom(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

async fn run_pg_dump(
    pg_dump_path: &str,
    database_url: &str,
) -> Result<Vec<u8>, SnapshotProviderError> {
    let output = Command::new(pg_dump_path)
        .arg(database_url)
        .arg("--format=custom")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|source| SnapshotProviderError::Spawn {
            path: pg_dump_path.to_string(),
            source,
        })?;
    if !output.status.success() {
        return Err(SnapshotProviderError::PgDumpFailed {
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(output.stdout)
}

/// Loads the per-installation symmetric key `path` holds, generating and
/// persisting a fresh 32-byte one on first run. Mirrors
/// `ackplane_bridge::load_or_generate_salt`'s exact generate-once-and-persist
/// shape (ADR-0098 decision 3's precedent); duplicated in this crate rather
/// than imported because `ackplane-server` depends on no downstream crate,
/// including `ackplane-bridge` (ADR-0082 clause 1), and this is a ~15-line
/// single-purpose helper, not shared business logic.
async fn load_or_generate_key(path: &Path) -> io::Result<[u8; KEY_BYTES]> {
    if let Ok(existing) = fs::read(path).await {
        if existing.len() == KEY_BYTES {
            let mut key = [0_u8; KEY_BYTES];
            key.copy_from_slice(&existing);
            return Ok(key);
        }
    }
    let mut key = [0_u8; KEY_BYTES];
    getrandom::getrandom(&mut key)
        .map_err(|error| io::Error::other(format!("could not generate a snapshot key: {error}")))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    write_atomically(path, &key).await?;
    Ok(key)
}

/// Writes via a sibling temp file and rename so a crash mid-write never
/// leaves a partially written artifact or key at `path`.
async fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp_path = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&temp_path).await?;
        file.write_all(bytes).await?;
        file.sync_all().await?;
    }
    fs::rename(&temp_path, path).await
}

/// Node ids embed a `namespace:hex` request id (see
/// `administration_store::model::hex_id`), and `:` is invalid in a Windows
/// filename (reserved for drive letters) even though it is fine on Linux and
/// macOS -- replaced so an artifact path stays valid on every platform this
/// toolchain runs on.
fn filesystem_safe_id(id: &str) -> String {
    id.replace(':', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn rehearsal_test_url() -> Option<String> {
        std::env::var("ACKPLANE_TEST_REHEARSAL_DATABASE_URL").ok()
    }

    /// Creates and drops an ephemeral database against `maintenance_url`, and
    /// returns its own connection url -- used so rehearsal integration tests
    /// never touch `ACKPLANE_TEST_DATABASE_URL`'s shared migration state:
    /// tampering a migration digest there could break every other test or
    /// fleet agent sharing that database.
    async fn with_ephemeral_database<Fut>(
        maintenance_url: &str,
        name_prefix: &str,
        body: impl FnOnce(String) -> Fut,
    ) where
        Fut: std::future::Future<Output = ()>,
    {
        let name = format!("{}_{}", name_prefix, unique_suffix());
        let (client, connection) = tokio_postgres::connect(maintenance_url, NoTls)
            .await
            .expect("a direct maintenance connection should succeed");
        tokio::spawn(async move {
            let _ = connection.await;
        });
        client
            .batch_execute(&format!("CREATE DATABASE \"{name}\""))
            .await
            .expect("creating the ephemeral fixture database should succeed");

        let url = with_dbname(maintenance_url, &name)
            .expect("the rehearsal test url should be a postgresql:// uri");
        body(url).await;

        let _ = client
            .batch_execute(&format!("DROP DATABASE IF EXISTS \"{name}\""))
            .await;
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
}
