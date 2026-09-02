//! Creating a sealed platform snapshot, and inspecting one.

use super::*;

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
pub(super) enum DecryptedArtifact {
    DigestMismatch,
    TooShort,
    DecryptionFailed,
    Ready(Vec<u8>),
}

pub(super) async fn decrypt_sealed_artifact(
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
