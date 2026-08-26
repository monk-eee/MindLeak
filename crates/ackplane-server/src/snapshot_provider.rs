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
};

use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng},
    AeadCore, XChaCha20Poly1305, XNonce,
};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{fs, io::AsyncWriteExt, process::Command};

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
}

impl SnapshotProviderConfig {
    pub const SNAPSHOT_DIR_ENV: &'static str = "ACKPLANE_SNAPSHOT_DIR";
    pub const KEY_PATH_ENV: &'static str = "ACKPLANE_SNAPSHOT_KEY_PATH";
    pub const PG_DUMP_PATH_ENV: &'static str = "ACKPLANE_PG_DUMP_PATH";
    pub const PG_RESTORE_PATH_ENV: &'static str = "ACKPLANE_PG_RESTORE_PATH";
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
        Some(Self {
            database_url,
            snapshot_dir,
            key_path,
            pg_dump_path,
            pg_restore_path,
        })
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
    let sealed = fs::read(artifact_path).await?;
    let actual_digest = Sha256::digest(&sealed).to_vec();
    if actual_digest != expected_digest {
        return Ok(InspectionReport {
            integrity_verified: false,
            decryption_verified: false,
            archive_valid: false,
            archive_entry_count: None,
            reason: "The artifact's digest no longer matches its recorded manifest digest."
                .to_string(),
        });
    }
    if sealed.len() <= NONCE_BYTES {
        return Ok(InspectionReport {
            integrity_verified: true,
            decryption_verified: false,
            archive_valid: false,
            archive_entry_count: None,
            reason: "The artifact is too short to contain a nonce and any ciphertext.".to_string(),
        });
    }

    let (nonce_bytes, ciphertext) = sealed.split_at(NONCE_BYTES);
    let key = load_or_generate_key(&config.key_path).await?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let plaintext = match cipher.decrypt(XNonce::from_slice(nonce_bytes), ciphertext) {
        Ok(plaintext) => plaintext,
        Err(_) => {
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
}
