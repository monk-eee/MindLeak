//! ADR-0121 decision 1: an immutable publication history recorded beside
//! the mutable active snapshot in [`super`]. A publish through `publish()`
//! never rewrites or deletes anything recorded here (decision 8).

use std::time::SystemTime;

use sha2::{Digest, Sha256};

use super::{ClauseSnapshot, ConstitutionStore, ConstitutionStoreError};

/// Decision 1: what `record_publication` accepts. Distinct from
/// `PublishConstitutionRequest` -- that request replaces the mutable active
/// snapshot; this one appends an immutable history entry beside it.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordConstitutionPublicationRequest {
    pub tenant_id: String,
    pub repository_id: String,
    pub version_id: String,
    pub schema_version: String,
    pub status: String,
    pub clauses: Vec<ClauseSnapshot>,
    pub source_reference: Option<String>,
    pub source_digest: Option<Vec<u8>>,
    pub published_at: SystemTime,
}

/// One immutable publication, as `get_publication`/`list_publications` return it.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstitutionPublication {
    pub tenant_id: String,
    pub repository_id: String,
    pub version_id: String,
    pub schema_version: String,
    pub status: String,
    pub clauses: Vec<ClauseSnapshot>,
    pub source_reference: Option<String>,
    pub source_digest: Option<Vec<u8>>,
    pub published_at: SystemTime,
}

/// Private wire shape for `ClauseSnapshot` -- kept separate from the public
/// type so the durable payload encoding does not couple to whatever derives
/// `ClauseSnapshot` needs for its other, non-serialized callers.
#[derive(serde::Serialize, serde::Deserialize)]
struct ClauseSnapshotWire {
    id: String,
    slug: String,
    kind: String,
    title: String,
    statement: String,
    status: String,
    consequence: Option<String>,
    scope: Option<String>,
    rationale: Option<String>,
}

impl From<&ClauseSnapshot> for ClauseSnapshotWire {
    fn from(clause: &ClauseSnapshot) -> Self {
        Self {
            id: clause.id.clone(),
            slug: clause.slug.clone(),
            kind: clause.kind.clone(),
            title: clause.title.clone(),
            statement: clause.statement.clone(),
            status: clause.status.clone(),
            consequence: clause.consequence.clone(),
            scope: clause.scope.clone(),
            rationale: clause.rationale.clone(),
        }
    }
}

impl From<ClauseSnapshotWire> for ClauseSnapshot {
    fn from(wire: ClauseSnapshotWire) -> Self {
        Self {
            id: wire.id,
            slug: wire.slug,
            kind: wire.kind,
            title: wire.title,
            statement: wire.statement,
            status: wire.status,
            consequence: wire.consequence,
            scope: wire.scope,
            rationale: wire.rationale,
        }
    }
}

/// The full bounded publication content, serialized as one payload so a
/// digest over it covers everything decision 1 says must not silently
/// change under the same `(tenant_id, repository_id, version_id)`, not just
/// the clause list.
#[derive(serde::Serialize, serde::Deserialize)]
struct PublicationPayloadV1 {
    schema_version: String,
    status: String,
    clauses: Vec<ClauseSnapshotWire>,
    source_reference: Option<String>,
    source_digest: Option<Vec<u8>>,
}

impl ConstitutionStore {
    /// ADR-0121 decision 1: record an immutable publication beside the
    /// mutable active snapshot above (decision 8 -- this expands the schema,
    /// it never rewrites `constitution_snapshots`/`constitution_clauses`). A
    /// byte-identical retry for the same `(tenant_id, repository_id,
    /// version_id)` succeeds idempotently; any other content under the same
    /// identity is refused as an immutability violation, never silently
    /// overwritten.
    pub async fn record_publication(
        &mut self,
        request: RecordConstitutionPublicationRequest,
    ) -> Result<(), ConstitutionStoreError> {
        if request.version_id.trim().is_empty() {
            return Err(ConstitutionStoreError::EmptyVersionId);
        }
        if request.schema_version.trim().is_empty() {
            return Err(ConstitutionStoreError::EmptySchemaVersion);
        }
        if request.status.trim().is_empty() {
            return Err(ConstitutionStoreError::EmptyStatus);
        }

        let payload = PublicationPayloadV1 {
            schema_version: request.schema_version.clone(),
            status: request.status.clone(),
            clauses: request
                .clauses
                .iter()
                .map(ClauseSnapshotWire::from)
                .collect(),
            source_reference: request.source_reference.clone(),
            source_digest: request.source_digest.clone(),
        };
        let payload_bytes =
            serde_json::to_vec(&payload).expect("PublicationPayloadV1 always serializes");
        let payload_digest = Sha256::digest(&payload_bytes).to_vec();

        let transaction = self.client.transaction().await?;
        let existing = transaction
            .query_opt(
                "SELECT payload_digest FROM constitution_publications \
                 WHERE tenant_id = $1 AND repository_id = $2 AND version_id = $3",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.version_id,
                ],
            )
            .await?;
        if let Some(row) = existing {
            let stored_digest: Vec<u8> = row.get(0);
            transaction.commit().await?;
            if stored_digest == payload_digest {
                return Ok(());
            }
            return Err(ConstitutionStoreError::PublicationImmutabilityViolation {
                version_id: request.version_id.clone(),
            });
        }

        transaction
            .execute(
                "INSERT INTO constitution_publications \
                 (tenant_id, repository_id, version_id, schema_version, status, payload, \
                  payload_digest, source_reference, source_digest, published_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.version_id,
                    &request.schema_version,
                    &request.status,
                    &payload_bytes,
                    &payload_digest,
                    &request.source_reference,
                    &request.source_digest,
                    &request.published_at,
                ],
            )
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// One immutable publication, deserialized back through the same typed
    /// shape it was recorded with.
    pub async fn get_publication(
        &self,
        tenant_id: &str,
        repository_id: &str,
        version_id: &str,
    ) -> Result<Option<ConstitutionPublication>, ConstitutionStoreError> {
        let row = self
            .client
            .query_opt(
                "SELECT payload, source_reference, source_digest, published_at \
                 FROM constitution_publications \
                 WHERE tenant_id = $1 AND repository_id = $2 AND version_id = $3",
                &[&tenant_id, &repository_id, &version_id],
            )
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let payload_bytes: Vec<u8> = row.get(0);
        let payload: PublicationPayloadV1 =
            serde_json::from_slice(&payload_bytes).map_err(|error| {
                ConstitutionStoreError::CorruptPublicationPayload {
                    version_id: version_id.to_string(),
                    detail: error.to_string(),
                }
            })?;
        Ok(Some(ConstitutionPublication {
            tenant_id: tenant_id.to_string(),
            repository_id: repository_id.to_string(),
            version_id: version_id.to_string(),
            schema_version: payload.schema_version,
            status: payload.status,
            clauses: payload
                .clauses
                .into_iter()
                .map(ClauseSnapshot::from)
                .collect(),
            source_reference: row.get(1),
            source_digest: row.get(2),
            published_at: row.get(3),
        }))
    }

    /// Every publication recorded for this tenant/repository, oldest first
    /// -- the version history decision 6's eventual Bridge read surface
    /// needs; this slice only proves the store can answer it.
    pub async fn list_publications(
        &self,
        tenant_id: &str,
        repository_id: &str,
    ) -> Result<Vec<ConstitutionPublication>, ConstitutionStoreError> {
        let rows = self
            .client
            .query(
                "SELECT version_id, payload, source_reference, source_digest, published_at \
                 FROM constitution_publications \
                 WHERE tenant_id = $1 AND repository_id = $2 \
                 ORDER BY published_at ASC",
                &[&tenant_id, &repository_id],
            )
            .await?;
        let mut publications = Vec::with_capacity(rows.len());
        for row in rows {
            let version_id: String = row.get(0);
            let payload_bytes: Vec<u8> = row.get(1);
            let payload: PublicationPayloadV1 =
                serde_json::from_slice(&payload_bytes).map_err(|error| {
                    ConstitutionStoreError::CorruptPublicationPayload {
                        version_id: version_id.clone(),
                        detail: error.to_string(),
                    }
                })?;
            publications.push(ConstitutionPublication {
                tenant_id: tenant_id.to_string(),
                repository_id: repository_id.to_string(),
                version_id,
                schema_version: payload.schema_version,
                status: payload.status,
                clauses: payload
                    .clauses
                    .into_iter()
                    .map(ClauseSnapshot::from)
                    .collect(),
                source_reference: row.get(2),
                source_digest: row.get(3),
                published_at: row.get(4),
            });
        }
        Ok(publications)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constitution_store::tests::{clause, store, unique_scope};
    use crate::constitution_store::PublishConstitutionRequest;

    fn publication_request(
        tenant_id: &str,
        repository_id: &str,
        version_id: &str,
    ) -> RecordConstitutionPublicationRequest {
        RecordConstitutionPublicationRequest {
            tenant_id: tenant_id.to_string(),
            repository_id: repository_id.to_string(),
            version_id: version_id.to_string(),
            schema_version: "v1".to_string(),
            status: "active".to_string(),
            clauses: vec![clause("clause-a")],
            source_reference: Some("docs/adr/0121-....md".to_string()),
            source_digest: Some(vec![1, 2, 3, 4]),
            published_at: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
        }
    }

    #[tokio::test]
    async fn a_recorded_publication_reads_back_unchanged() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("publication-roundtrip");
        let request = publication_request(&tenant_id, &repository_id, "version-1");

        store.record_publication(request.clone()).await.unwrap();

        let fetched = store
            .get_publication(&tenant_id, &repository_id, "version-1")
            .await
            .unwrap()
            .expect("a recorded publication should be readable");
        assert_eq!(fetched.schema_version, request.schema_version);
        assert_eq!(fetched.status, request.status);
        assert_eq!(fetched.clauses, request.clauses);
        assert_eq!(fetched.source_reference, request.source_reference);
        assert_eq!(fetched.source_digest, request.source_digest);
    }

    #[tokio::test]
    async fn get_publication_returns_none_for_an_unknown_version() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("publication-missing");

        let fetched = store
            .get_publication(&tenant_id, &repository_id, "version-does-not-exist")
            .await
            .unwrap();
        assert_eq!(fetched, None);
    }

    #[tokio::test]
    async fn recording_the_same_publication_twice_is_an_idempotent_no_op() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("publication-replay");
        let request = publication_request(&tenant_id, &repository_id, "version-1");

        store.record_publication(request.clone()).await.unwrap();
        store
            .record_publication(request.clone())
            .await
            .expect("a byte-identical replay must succeed, not error");

        let publications = store
            .list_publications(&tenant_id, &repository_id)
            .await
            .unwrap();
        assert_eq!(publications.len(), 1, "a replay must not duplicate the row");
    }

    #[tokio::test]
    async fn recording_different_content_under_the_same_version_id_is_rejected() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("publication-mutate");
        let mut request = publication_request(&tenant_id, &repository_id, "version-1");
        store.record_publication(request.clone()).await.unwrap();

        request.status = "retired".to_string();
        let error = store.record_publication(request).await.unwrap_err();
        assert!(matches!(
            error,
            ConstitutionStoreError::PublicationImmutabilityViolation { .. }
        ));

        // The original content must survive the rejected mutation attempt.
        let fetched = store
            .get_publication(&tenant_id, &repository_id, "version-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, "active");
    }

    #[tokio::test]
    async fn a_republish_through_publish_never_touches_recorded_publication_history() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("publication-independent");
        store
            .record_publication(publication_request(&tenant_id, &repository_id, "version-1"))
            .await
            .unwrap();

        // decision 8: the mutable active-snapshot path must not disturb the
        // immutable history recorded through record_publication.
        store
            .publish(PublishConstitutionRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                version_id: "version-2".to_string(),
                version: 2,
                status: "active".to_string(),
                clauses: vec![clause("clause-z")],
            })
            .await
            .unwrap();

        let history = store
            .get_publication(&tenant_id, &repository_id, "version-1")
            .await
            .unwrap()
            .expect("publish() must not delete an earlier recorded publication");
        assert_eq!(history.status, "active");
    }

    #[tokio::test]
    async fn list_publications_orders_oldest_first() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("publication-list");
        let mut first = publication_request(&tenant_id, &repository_id, "version-1");
        first.published_at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000);
        let mut second = publication_request(&tenant_id, &repository_id, "version-2");
        second.published_at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(2_000);

        // Recorded out of chronological order; the list must still sort by
        // published_at, not insertion order.
        store.record_publication(second).await.unwrap();
        store.record_publication(first).await.unwrap();

        let publications = store
            .list_publications(&tenant_id, &repository_id)
            .await
            .unwrap();
        let ids: Vec<&str> = publications
            .iter()
            .map(|publication| publication.version_id.as_str())
            .collect();
        assert_eq!(ids, vec!["version-1", "version-2"]);
    }
}
