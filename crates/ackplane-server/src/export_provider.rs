//! Builds a bounded, redacted Export artifact (ADR-0119 decision 5).
//!
//! This is the one place in `ackplane-server` that queries a data category's
//! own table for Export purposes and writes the result to disk;
//! `administration_store` only ever records the immutable request and
//! receipt this module's outcome produces -- the same separation
//! `snapshot_provider` keeps from `administration_store`. Deliberately one
//! closed data category today (`telemetry_events`, the same one Lifecycle
//! purge acts on): bounded diagnostic history whose internal identifiers
//! (`telemetry_id`, `node_id`, `agent_session_id`) are exactly what a
//! portability or audit export should redact rather than disclose.
//!
//! Unlike a Snapshot artifact, an export is not encrypted: decision 5 never
//! requires it, and the whole point of this artifact is that it is *already*
//! bounded and redacted, not a second copy of production state needing the
//! same custody controls as a full-database backup.

use std::{io, path::PathBuf, time::SystemTime};

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{fs, io::AsyncWriteExt};
use tokio_postgres::NoTls;

/// The schema version this module's own JSON shape carries. Bump this, not
/// the redaction list silently, if the exported shape ever changes -- ADR-0119
/// decision 5 requires an export contract to fix its schema version.
pub const TELEMETRY_EXPORT_SCHEMA_VERSION: &str = "telemetry-export-v1";

/// Where an Export artifact is written. Resolved once from environment at
/// Bridge/service startup, mirroring `SnapshotProviderConfig`'s shape.
pub struct ExportProviderConfig {
    pub database_url: String,
    pub export_dir: PathBuf,
}

impl ExportProviderConfig {
    pub const EXPORT_DIR_ENV: &'static str = "ACKPLANE_EXPORT_DIR";

    /// `None` when `ACKPLANE_EXPORT_DIR` is unset -- Export is then
    /// unavailable rather than falling back to a guessed location, the same
    /// "refuse, never invent a default" rule `SnapshotProviderConfig::resolve`
    /// already applies.
    pub fn resolve(lookup: impl Fn(&str) -> Option<String>, database_url: String) -> Option<Self> {
        let value = |key: &str| {
            lookup(key)
                .map(|raw| raw.trim().to_string())
                .filter(|raw| !raw.is_empty())
        };
        let export_dir = PathBuf::from(value(Self::EXPORT_DIR_ENV)?);
        Some(Self {
            database_url,
            export_dir,
        })
    }
}

/// What a successful Export execution durably records (ADR-0119 decision 5's
/// receipt fields, minus what the caller already knows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportArtifact {
    pub artifact_path: String,
    pub manifest_digest: Vec<u8>,
    pub record_count: i64,
    pub redacted_fields: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ExportProviderError {
    #[error("could not prepare the export directory or artifact: {0}")]
    Io(#[source] io::Error),
    #[error("could not connect to query the export data category: {0}")]
    Database(#[from] tokio_postgres::Error),
}

impl From<io::Error> for ExportProviderError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// The bounded, redacted shape a `telemetry_events` export produces. Every
/// internal identifier (`telemetry_id`, `node_id`, `agent_session_id`) is
/// deliberately absent -- see this module's own doc comment.
#[derive(Serialize)]
struct TelemetryExportDocument {
    schema_version: &'static str,
    tenant_id: String,
    repository_id: String,
    purpose: String,
    redacted_fields: Vec<&'static str>,
    generated_at_seconds: u64,
    records: Vec<TelemetryExportRecord>,
}

#[derive(Serialize)]
struct TelemetryExportRecord {
    kind: i16,
    name: String,
    outcome: i16,
    duration_ms: i64,
    occurred_at_seconds: u64,
    measurements: serde_json::Value,
}

const REDACTED_TELEMETRY_FIELDS: [&str; 3] = ["telemetry_id", "node_id", "agent_session_id"];

/// Queries at most `max_records` `telemetry_events` rows for
/// `tenant_id`/`repository_id`, redacts every internal identifier, and
/// writes the bounded result under `export_dir`.
pub async fn create_telemetry_export(
    config: &ExportProviderConfig,
    request_id: &str,
    tenant_id: &str,
    repository_id: &str,
    purpose: &str,
    max_records: u32,
) -> Result<ExportArtifact, ExportProviderError> {
    fs::create_dir_all(&config.export_dir).await?;

    let (client, connection) = tokio_postgres::connect(&config.database_url, NoTls).await?;
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            tracing::error!(%error, "ackplane Export provider connection closed with an error");
        }
    });

    let limit = i64::from(max_records);
    let rows = client
        .query(
            "SELECT kind, name, outcome, duration_ms, occurred_at, measurements \
             FROM telemetry_events \
             WHERE tenant_id = $1 AND repository_id = $2 \
             ORDER BY occurred_at DESC LIMIT $3",
            &[&tenant_id, &repository_id, &limit],
        )
        .await?;

    let records: Vec<TelemetryExportRecord> = rows
        .iter()
        .map(|row| {
            let occurred_at: SystemTime = row.get("occurred_at");
            let measurements_json: String = row.get("measurements");
            TelemetryExportRecord {
                kind: row.get("kind"),
                name: row.get("name"),
                outcome: row.get("outcome"),
                duration_ms: row.get("duration_ms"),
                occurred_at_seconds: unix_seconds(occurred_at),
                measurements: serde_json::from_str(&measurements_json)
                    .unwrap_or(serde_json::Value::Null),
            }
        })
        .collect();
    let record_count = i64::try_from(records.len()).unwrap_or(i64::MAX);

    let document = TelemetryExportDocument {
        schema_version: TELEMETRY_EXPORT_SCHEMA_VERSION,
        tenant_id: tenant_id.to_string(),
        repository_id: repository_id.to_string(),
        purpose: purpose.to_string(),
        redacted_fields: REDACTED_TELEMETRY_FIELDS.to_vec(),
        generated_at_seconds: unix_seconds(SystemTime::now()),
        records,
    };
    let body = serde_json::to_vec_pretty(&document)
        .map_err(|error| ExportProviderError::Io(io::Error::other(error)))?;
    let manifest_digest = Sha256::digest(&body).to_vec();

    let artifact_path = config
        .export_dir
        .join(format!("{}.json", filesystem_safe_id(request_id)));
    write_atomically(&artifact_path, &body).await?;

    Ok(ExportArtifact {
        artifact_path: artifact_path.to_string_lossy().into_owned(),
        manifest_digest,
        record_count,
        redacted_fields: REDACTED_TELEMETRY_FIELDS
            .iter()
            .map(|field| field.to_string())
            .collect(),
    })
}

fn unix_seconds(timestamp: SystemTime) -> u64 {
    timestamp
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Writes via a sibling temp file and rename so a crash mid-write never
/// leaves a partially written artifact at `path`. Mirrors
/// `snapshot_provider::write_atomically` exactly; duplicated rather than
/// shared because it is a ~10-line, single-purpose helper and each module
/// already keeps its own local constants this way.
async fn write_atomically(path: &std::path::Path, bytes: &[u8]) -> io::Result<()> {
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
/// toolchain runs on. Duplicated from `snapshot_provider` for the same
/// reason `write_atomically` is: a one-line helper, not shared business
/// logic.
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
    fn resolve_returns_none_without_an_export_dir() {
        let config = ExportProviderConfig::resolve(|_| None, "postgresql://x".to_string());
        assert!(config.is_none());
    }

    #[test]
    fn resolve_uses_the_configured_export_dir() {
        let config = ExportProviderConfig::resolve(
            |key| (key == ExportProviderConfig::EXPORT_DIR_ENV).then(|| "/exports".to_string()),
            "postgresql://x".to_string(),
        )
        .expect("an export dir alone should resolve");
        assert_eq!(config.export_dir, PathBuf::from("/exports"));
    }

    #[tokio::test]
    async fn create_telemetry_export_redacts_internal_identifiers_and_bounds_records() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let suffix = crate::test_support::unique_id("export-provider-test");
        let tenant_id = format!("tenant-{suffix}");
        let repository_id = format!("repository-{suffix}");

        let (client, connection) = tokio_postgres::connect(&database_url, NoTls)
            .await
            .expect("the test database should accept a direct fixture connection");
        tokio::spawn(async move {
            let _ = connection.await;
        });
        for index in 0..3 {
            client
                .execute(
                    "INSERT INTO telemetry_events (tenant_id, repository_id, telemetry_id, \
                         node_id, agent_session_id, kind, name, outcome, duration_ms, occurred_at) \
                     VALUES ($1,$2,$3,'a-node-id','a-session-id',1,'a-tool-name',1,42,now())",
                    &[
                        &tenant_id,
                        &repository_id,
                        &format!("event-{suffix}-{index}"),
                    ],
                )
                .await
                .expect("inserting a telemetry event fixture should succeed");
        }

        let dir = temp_dir("ackplane-export-provider-test");
        let _ = fs::remove_dir_all(&dir).await;
        let config = ExportProviderConfig {
            database_url,
            export_dir: dir.clone(),
        };

        let artifact = create_telemetry_export(
            &config,
            "test-export-request",
            &tenant_id,
            &repository_id,
            "test purpose",
            2,
        )
        .await
        .expect("a real bounded export query should succeed");
        assert_eq!(
            artifact.record_count, 2,
            "max_records=2 should bound the result"
        );
        assert_eq!(artifact.manifest_digest.len(), 32);
        assert_eq!(
            artifact.redacted_fields,
            vec!["telemetry_id", "node_id", "agent_session_id"]
        );

        let body = fs::read_to_string(&artifact.artifact_path)
            .await
            .expect("the export artifact file should exist");
        assert!(!body.contains("a-node-id"));
        assert!(!body.contains("a-session-id"));
        assert!(!body.contains(&format!("event-{suffix}")));
        assert!(body.contains("a-tool-name"));
        assert_eq!(
            Sha256::digest(body.as_bytes()).to_vec(),
            artifact.manifest_digest
        );

        let _ = fs::remove_dir_all(&dir).await;
    }
}
