//! Tenant-scoped Fleet read models for the Bridge (ADR-0095).
//!
//! Fleet rows are derived from active enrollment receipts and the graph
//! projection's freshness state. This module is read-only: neither browser
//! views nor their API may change enrolment, claims, or ledger records.

use std::time::SystemTime;

use tokio_postgres::{Client, NoTls};

const ENROLLMENT_MIGRATION: &str = include_str!("../migrations/0003_enrollment.sql");
const PROJECTION_MIGRATION: &str = include_str!("../migrations/0002_projection.sql");
const LEDGER_MIGRATION: &str = include_str!("../migrations/0001_ledger.sql");

/// One repository represented in a tenant's Fleet view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FleetRepository {
    pub repository_id: String,
    pub active_node_count: i64,
    pub last_activated_at: SystemTime,
    pub ledger_stream_position: i64,
    pub projection_stream_position: Option<i64>,
    pub projection_updated_at: Option<SystemTime>,
    pub freshness: RepositoryFreshness,
}

/// Whether a repository's graph projection has caught up with its ledger,
/// as read by the Bridge (ADR-0095 decision 6: "a rebuilding or lagging
/// projection is presented as such" rather than silently reported as
/// current). Distinct from [`crate::projection::ProjectionFreshness`], which
/// is the raw `(stream_position, projected_at)` pair the projector itself
/// reads; this type is the Bridge's classification of that pair against the
/// ledger head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryFreshness {
    /// The repository has an active enrollment but has never produced a
    /// projection.
    NeverProjected,
    /// The projection exists but has not caught up with the ledger's current
    /// stream position.
    Lagging,
    /// The projection's stream position matches the ledger head.
    Fresh,
}

/// Classify a projection's currency against the ledger head (ADR-0095
/// decision 6), shared by every read that reports freshness so the Fleet
/// list and a single repository's detail can never disagree about what
/// "lagging" means.
fn classify_freshness(
    ledger_stream_position: i64,
    projection_stream_position: Option<i64>,
) -> RepositoryFreshness {
    match projection_stream_position {
        None => RepositoryFreshness::NeverProjected,
        Some(projected) if projected < ledger_stream_position => RepositoryFreshness::Lagging,
        Some(_) => RepositoryFreshness::Fresh,
    }
}

/// One repository's coordination, ledger, and projection state (ADR-0095
/// decision 4's repository detail resource).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryDetail {
    pub repository_id: String,
    pub active_node_count: i64,
    pub last_activated_at: SystemTime,
    pub ledger_stream_position: i64,
    pub projection_stream_position: Option<i64>,
    pub projection_updated_at: Option<SystemTime>,
    pub freshness: RepositoryFreshness,
}

/// One accepted ledger record surfaced on a repository's timeline (ADR-0095
/// decision 4's timeline resource). Rejections are not persisted anywhere the
/// Bridge can read (ADR-0086), so a timeline names only what was accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineEvent {
    pub stream_position: i64,
    pub occurred_at: SystemTime,
    pub payload_type: String,
    pub producer_id: String,
}

/// Read-only access to Fleet summaries derived from Ackplane's accepted state.
pub struct FleetStore {
    client: Client,
}

impl FleetStore {
    /// Connect to the Ackplane read models needed for Fleet queries.
    pub async fn connect(database_url: &str) -> Result<Self, tokio_postgres::Error> {
        let (mut client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!(%error, "ackplane fleet connection closed with an error");
            }
        });
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::ENROLLMENT,
            ENROLLMENT_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::PROJECTION,
            PROJECTION_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::LEDGER,
            LEDGER_MIGRATION,
        )
        .await?;
        Ok(Self { client })
    }

    /// Return repositories with an active enrollment receipt in one tenant.
    ///
    /// A missing projection state remains visible as `None`; it means the
    /// repository is enrolled but has not produced a graph projection yet.
    pub async fn repositories(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<FleetRepository>, tokio_postgres::Error> {
        let rows = self
            .client
            .query(
                "SELECT request.repository_id, count(*)::BIGINT, max(receipt.activated_at), \
                        COALESCE(head.position, 0), \
                        state.stream_position, state.projected_at \
                 FROM enrollment_requests AS request \
                 INNER JOIN enrollment_receipts AS receipt \
                    ON receipt.tenant_id = request.tenant_id \
                   AND receipt.repository_id = request.repository_id \
                   AND receipt.request_id = request.request_id \
                 LEFT JOIN projection_state AS state \
                    ON state.tenant_id = request.tenant_id \
                   AND state.repository_id = request.repository_id \
                 LEFT JOIN stream_heads AS head \
                    ON head.tenant_id = request.tenant_id \
                   AND head.repository_id = request.repository_id \
                 WHERE request.tenant_id = $1 \
                 GROUP BY request.repository_id, head.position, \
                          state.stream_position, state.projected_at \
                 ORDER BY request.repository_id ASC",
                &[&tenant_id],
            )
            .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let ledger_stream_position: i64 = row.get(3);
                let projection_stream_position: Option<i64> = row.get(4);
                FleetRepository {
                    repository_id: row.get(0),
                    active_node_count: row.get(1),
                    last_activated_at: row.get(2),
                    ledger_stream_position,
                    freshness: classify_freshness(
                        ledger_stream_position,
                        projection_stream_position,
                    ),
                    projection_stream_position,
                    projection_updated_at: row.get(5),
                }
            })
            .collect())
    }

    /// One repository's coordination, ledger, and projection state, scoped to
    /// a tenant (ADR-0095 decision 4). `None` when the repository has no
    /// active enrollment receipt in that tenant — a caller that cannot see a
    /// repository must never distinguish "not enrolled" from "enrolled in a
    /// different tenant"; both read the same way here.
    pub async fn repository(
        &self,
        tenant_id: &str,
        repository_id: &str,
    ) -> Result<Option<RepositoryDetail>, tokio_postgres::Error> {
        let row = self
            .client
            .query_opt(
                "SELECT request.repository_id, count(*)::BIGINT, max(receipt.activated_at), \
                        COALESCE(head.position, 0), \
                        state.stream_position, state.projected_at \
                 FROM enrollment_requests AS request \
                 INNER JOIN enrollment_receipts AS receipt \
                    ON receipt.tenant_id = request.tenant_id \
                   AND receipt.repository_id = request.repository_id \
                   AND receipt.request_id = request.request_id \
                 LEFT JOIN projection_state AS state \
                    ON state.tenant_id = request.tenant_id \
                   AND state.repository_id = request.repository_id \
                 LEFT JOIN stream_heads AS head \
                    ON head.tenant_id = request.tenant_id \
                   AND head.repository_id = request.repository_id \
                 WHERE request.tenant_id = $1 AND request.repository_id = $2 \
                 GROUP BY request.repository_id, head.position, \
                          state.stream_position, state.projected_at",
                &[&tenant_id, &repository_id],
            )
            .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let ledger_stream_position: i64 = row.get(3);
        let projection_stream_position: Option<i64> = row.get(4);
        let freshness = classify_freshness(ledger_stream_position, projection_stream_position);

        Ok(Some(RepositoryDetail {
            repository_id: row.get(0),
            active_node_count: row.get(1),
            last_activated_at: row.get(2),
            ledger_stream_position,
            projection_stream_position,
            projection_updated_at: row.get(5),
            freshness,
        }))
    }

    /// A repository's most recent accepted ledger records, newest first
    /// (ADR-0095 decision 4's timeline resource). Only accepted records are
    /// ever persisted (ADR-0086), so this names accepted positions, never
    /// rejections.
    pub async fn timeline(
        &self,
        tenant_id: &str,
        repository_id: &str,
        limit: i64,
    ) -> Result<Vec<TimelineEvent>, tokio_postgres::Error> {
        let rows = self
            .client
            .query(
                "SELECT stream_position, occurred_at, payload_type, producer_id \
                 FROM ledger_records \
                 WHERE tenant_id = $1 AND repository_id = $2 \
                 ORDER BY stream_position DESC \
                 LIMIT $3",
                &[&tenant_id, &repository_id, &limit],
            )
            .await?;

        Ok(rows
            .into_iter()
            .map(|row| TimelineEvent {
                stream_position: row.get(0),
                occurred_at: row.get(1),
                payload_type: row.get(2),
                producer_id: row.get(3),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::{
        enrollment::{activation_challenge_bytes, public_key_fingerprint},
        enrollment_store::{
            ActivationChallengeRequest, EnrollmentActivation, EnrollmentApproval, EnrollmentStore,
            EnrollmentSubmission,
        },
        ledger::{DedupKey, EventEnvelope, LedgerStore, ProvenanceClass},
        projection::{Projector, StructuralFact},
        test_support::uuid_ish,
    };

    /// Enrolls and activates one node for a fresh tenant/repository pair, so
    /// each test starts from an active Fleet entry without repeating the
    /// enrolment ceremony inline.
    async fn enroll_and_activate(database_url: &str, unique_id: &str) -> (String, String) {
        let signing_key = SigningKey::from_bytes(&[11; 32]);
        let tenant_id = format!("fleet-tenant-{unique_id}");
        let repository_id = format!("fleet-repository-{unique_id}");
        let public_key = signing_key.verifying_key().to_bytes();
        let submission = EnrollmentSubmission {
            request_id: format!("fleet-request-{unique_id}"),
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            proposed_node_id: format!("fleet-node-{unique_id}"),
            display_name: "Fleet test node".to_string(),
            public_key: public_key.to_vec(),
            public_key_fingerprint: public_key_fingerprint(&public_key),
            requested_capabilities: vec!["synchronize".to_string()],
            created_at: "2026-01-01T00:00:00Z".to_string(),
            expires_at: "2030-01-01T00:00:00Z".to_string(),
        };
        let request = ActivationChallengeRequest {
            request_id: submission.request_id.clone(),
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            proposed_node_id: submission.proposed_node_id.clone(),
            public_key_fingerprint: submission.public_key_fingerprint.clone(),
        };
        let now = SystemTime::now();
        let mut enrollment = EnrollmentStore::connect(database_url)
            .await
            .expect("connect enrollment store");
        enrollment
            .submit(&submission)
            .await
            .expect("submit enrollment");
        enrollment
            .approve(&EnrollmentApproval {
                request_id: submission.request_id.clone(),
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                public_key_fingerprint: submission.public_key_fingerprint.clone(),
                approved_capabilities: submission.requested_capabilities.clone(),
                approved_by: "fleet-test-administrator".to_string(),
            })
            .await
            .expect("approve enrollment");
        // `activation_challenges.nonce` carries a GLOBAL unique constraint
        // (not scoped per tenant/repo), so a hardcoded literal collides the
        // moment this helper is called from more than one test in the same
        // process. Derive it from `unique_id` instead.
        let nonce: [u8; 32] = Sha256::digest(unique_id.as_bytes()).into();
        let challenge = enrollment
            .issue_challenge(&request, &nonce, now)
            .await
            .expect("issue activation challenge");
        let signature = signing_key.sign(&activation_challenge_bytes(
            &challenge.nonce,
            &request.request_id,
            &request.tenant_id,
            &request.repository_id,
            &request.proposed_node_id,
            &request.public_key_fingerprint,
        ));
        enrollment
            .activate(
                &EnrollmentActivation {
                    request,
                    nonce: challenge.nonce,
                    signature: signature.to_bytes().to_vec(),
                },
                &format!("fleet-receipt-{unique_id}"),
                &format!("fleet-signing-key-{unique_id}"),
                now,
            )
            .await
            .expect("activate enrollment");

        (tenant_id, repository_id)
    }

    #[tokio::test]
    async fn an_enrolled_repository_without_a_projection_remains_visible_in_fleet() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;

        let fleet = FleetStore::connect(&database_url)
            .await
            .expect("connect fleet store");
        let repositories = fleet
            .repositories(&tenant_id)
            .await
            .expect("query fleet repositories");

        assert_eq!(repositories.len(), 1);
        let repository = &repositories[0];
        assert_eq!(repository.repository_id, repository_id);
        assert_eq!(repository.active_node_count, 1);
        assert_eq!(repository.projection_stream_position, None);
        assert_eq!(repository.projection_updated_at, None);
    }

    #[tokio::test]
    async fn a_repository_not_enrolled_in_the_tenant_is_invisible_to_repository_detail() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;

        let fleet = FleetStore::connect(&database_url)
            .await
            .expect("connect fleet store");

        // Right repository, wrong tenant: must read exactly like "not
        // enrolled", never like a cross-tenant peek.
        let wrong_tenant = fleet
            .repository(&format!("{tenant_id}-other"), &repository_id)
            .await
            .expect("query repository detail");
        assert!(wrong_tenant.is_none());

        // Right tenant, a repository id nobody ever enrolled.
        let never_enrolled = fleet
            .repository(&tenant_id, &format!("{repository_id}-other"))
            .await
            .expect("query repository detail");
        assert!(never_enrolled.is_none());
    }

    #[tokio::test]
    async fn repository_detail_reports_lagging_when_the_ledger_outruns_the_projection() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;

        let mut ledger = LedgerStore::connect(&database_url)
            .await
            .expect("connect ledger store");
        let envelope = EventEnvelope {
            key: DedupKey {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                producer_id: format!("fleet-producer-{unique_id}"),
                producer_sequence: 1,
            },
            payload: b"{}".to_vec(),
            payload_digest: vec![1, 2, 3],
            schema_version: "v1".to_string(),
            occurred_at: SystemTime::now(),
            payload_type: "structural_fact".to_string(),
            previous_envelope_digest: None,
            signing_key_id: None,
            signature: None,
            provenance: ProvenanceClass::EnrolledNode,
        };
        ledger.append(&envelope).await.expect("append envelope");

        let fleet = FleetStore::connect(&database_url)
            .await
            .expect("connect fleet store");
        let detail = fleet
            .repository(&tenant_id, &repository_id)
            .await
            .expect("query repository detail")
            .expect("repository is enrolled");

        assert_eq!(detail.ledger_stream_position, 1);
        assert_eq!(detail.projection_stream_position, None);
        assert_eq!(detail.freshness, RepositoryFreshness::NeverProjected);

        let timeline = fleet
            .timeline(&tenant_id, &repository_id, 50)
            .await
            .expect("query repository timeline");
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0].stream_position, 1);
        assert_eq!(timeline[0].payload_type, "structural_fact");
        assert_eq!(timeline[0].producer_id, envelope.key.producer_id);
    }

    #[tokio::test]
    async fn repository_detail_reports_fresh_once_the_projection_catches_up() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;

        let mut ledger = LedgerStore::connect(&database_url)
            .await
            .expect("connect ledger store");
        let fact = StructuralFact {
            node_id: format!("artifact:fleet-{unique_id}.rs"),
            node_type: "artifact".to_string(),
            label: format!("fleet-{unique_id}.rs"),
            edges: Vec::new(),
        };
        let envelope = EventEnvelope {
            key: DedupKey {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                producer_id: format!("fleet-producer-{unique_id}"),
                producer_sequence: 1,
            },
            payload: serde_json::to_vec(&fact).expect("encode structural fact"),
            payload_digest: vec![4, 5, 6],
            schema_version: "v1".to_string(),
            occurred_at: SystemTime::now(),
            payload_type: "structural_fact".to_string(),
            previous_envelope_digest: None,
            signing_key_id: None,
            signature: None,
            provenance: ProvenanceClass::EnrolledNode,
        };
        ledger.append(&envelope).await.expect("append envelope");

        let mut projector = Projector::connect(&database_url)
            .await
            .expect("connect projector");
        projector
            .rebuild(&tenant_id, &repository_id)
            .await
            .expect("rebuild projection");

        let fleet = FleetStore::connect(&database_url)
            .await
            .expect("connect fleet store");
        let detail = fleet
            .repository(&tenant_id, &repository_id)
            .await
            .expect("query repository detail")
            .expect("repository is enrolled");

        assert_eq!(detail.ledger_stream_position, 1);
        assert_eq!(detail.projection_stream_position, Some(1));
        assert_eq!(detail.freshness, RepositoryFreshness::Fresh);
    }
}
