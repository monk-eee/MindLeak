//! Rehearsing a recovery against scratch, and executing one.

use super::*;

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

/// ADR-0145 decision 4-7: runs the real, destructive restore against
/// `config.database_url` -- the authoritative production database, never a
/// scratch target. Callers must already have satisfied every gate this
/// function does not itself re-check: a `Confirmed` authorization (ADR-0134's
/// dual-signing-key pattern), a fresh passing rehearsal of this exact digest
/// (decision 3), and `single_tenant_attested` (decision 6, checked first here
/// as the one gate this function is positioned to enforce directly, since
/// the config it already holds is the config a rehearsal-freshness check has
/// no other reason to see).
///
/// Returns `Ok(report)` even when `pg_restore` itself fails -- a failed
/// restore is durable evidence the caller must record as a `Failed` receipt,
/// never a silent success or a raw error indistinguishable from a
/// pre-flight refusal (ADR-0119 decision 10: no silent fallback, no retry
/// with checks relaxed). `Err` is reserved for what happens *before* any
/// destructive attempt: an unattested deployment, or the artifact itself
/// could not be read, decrypted, or written to a temp file.
pub async fn execute_recovery(
    config: &SnapshotProviderConfig,
    artifact_path: &str,
    expected_digest: &[u8],
) -> Result<RecoveryExecutionRestoreReport, SnapshotProviderError> {
    config.ensure_recovery_execution_permitted()?;

    let plaintext = match decrypt_sealed_artifact(config, artifact_path, expected_digest).await? {
        DecryptedArtifact::DigestMismatch => {
            return Ok(RecoveryExecutionRestoreReport {
                restore_duration_ms: 0,
                succeeded: false,
                reason: "The artifact's digest no longer matches its recorded manifest digest."
                    .to_string(),
            })
        }
        DecryptedArtifact::TooShort => {
            return Ok(RecoveryExecutionRestoreReport {
                restore_duration_ms: 0,
                succeeded: false,
                reason: "The artifact is too short to contain a nonce and any ciphertext."
                    .to_string(),
            })
        }
        DecryptedArtifact::DecryptionFailed => {
            return Ok(RecoveryExecutionRestoreReport {
                restore_duration_ms: 0,
                succeeded: false,
                reason:
                    "The artifact could not be decrypted with this installation's snapshot key."
                        .to_string(),
            })
        }
        DecryptedArtifact::Ready(plaintext) => plaintext,
    };

    let temp_path = std::env::temp_dir().join(format!(
        "ackplane-recovery-execution-{}-{}.pgdump",
        std::process::id(),
        unique_suffix()
    ));
    fs::write(&temp_path, &plaintext).await?;

    let restore_started = Instant::now();
    let output = Command::new(&config.pg_restore_path)
        // Restoring in place, over an already-migrated authoritative
        // database: existing objects must be dropped before being recreated,
        // unlike rehearsal's restore into a freshly created, empty scratch
        // database.
        .arg("--clean")
        .arg("--if-exists")
        .arg("--no-owner")
        .arg("-d")
        .arg(&config.database_url)
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
    let restore_duration_ms =
        i64::try_from(restore_started.elapsed().as_millis()).unwrap_or(i64::MAX);

    if output.status.success() {
        Ok(RecoveryExecutionRestoreReport {
            restore_duration_ms,
            succeeded: true,
            reason: format!("pg_restore completed against the authoritative database in {restore_duration_ms}ms."),
        })
    } else {
        Ok(RecoveryExecutionRestoreReport {
            restore_duration_ms,
            succeeded: false,
            reason: format!(
                "pg_restore against the authoritative database failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        })
    }
}

/// ADR-0145 decision 3's freshness gate: a rehearsal proves nothing about
/// compatibility with whatever migrations landed since it ran, so a rehearsal
/// older than [`MAX_REHEARSAL_FRESHNESS`] must not admit production execution
/// -- however recently it passed relative to *some* earlier schema.
pub fn rehearsal_is_fresh(rehearsal_occurred_at: SystemTime, now: SystemTime) -> bool {
    match now.duration_since(rehearsal_occurred_at) {
        Ok(age) => age <= MAX_REHEARSAL_FRESHNESS,
        // A rehearsal timestamped in the future relative to `now` is not
        // trustworthy evidence either -- refuse rather than treat clock skew
        // as infinite freshness.
        Err(_) => false,
    }
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
