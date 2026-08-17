//! Tenant-scoped Fleet read models for the Bridge (ADR-0095).
//!
//! Fleet rows are derived from active enrollment receipts and the graph
//! projection's freshness state. This module is read-only: neither browser
//! views nor their API may change enrolment, claims, or ledger records.

use std::time::SystemTime;

use tokio_postgres::{Client, NoTls};

const ENROLLMENT_MIGRATION: &str = include_str!("../migrations/0003_enrollment.sql");
const PROJECTION_MIGRATION: &str = include_str!("../migrations/0002_projection.sql");

/// One repository represented in a tenant's Fleet view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FleetRepository {
    pub repository_id: String,
    pub active_node_count: i64,
    pub last_activated_at: SystemTime,
    pub projection_stream_position: Option<i64>,
    pub projection_updated_at: Option<SystemTime>,
}

/// Read-only access to Fleet summaries derived from Ackplane's accepted state.
pub struct FleetStore {
    client: Client,
}

impl FleetStore {
    /// Connect to the Ackplane read models needed for Fleet queries.
    pub async fn connect(database_url: &str) -> Result<Self, tokio_postgres::Error> {
        let (client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!(%error, "ackplane fleet connection closed with an error");
            }
        });
        client.batch_execute(ENROLLMENT_MIGRATION).await?;
        client.batch_execute(PROJECTION_MIGRATION).await?;
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
                        state.stream_position, state.projected_at \
                 FROM enrollment_requests AS request \
                 INNER JOIN enrollment_receipts AS receipt \
                    ON receipt.tenant_id = request.tenant_id \
                   AND receipt.repository_id = request.repository_id \
                   AND receipt.request_id = request.request_id \
                 LEFT JOIN projection_state AS state \
                    ON state.tenant_id = request.tenant_id \
                   AND state.repository_id = request.repository_id \
                 WHERE request.tenant_id = $1 \
                 GROUP BY request.repository_id, state.stream_position, state.projected_at \
                 ORDER BY request.repository_id ASC",
                &[&tenant_id],
            )
            .await?;

        Ok(rows
            .into_iter()
            .map(|row| FleetRepository {
                repository_id: row.get(0),
                active_node_count: row.get(1),
                last_activated_at: row.get(2),
                projection_stream_position: row.get(3),
                projection_updated_at: row.get(4),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use ed25519_dalek::{Signer, SigningKey};

    use super::*;
    use crate::{
        enrollment::{activation_challenge_bytes, public_key_fingerprint},
        enrollment_store::{
            ActivationChallengeRequest, EnrollmentActivation, EnrollmentApproval, EnrollmentStore,
            EnrollmentSubmission,
        },
        test_support::uuid_ish,
    };

    #[tokio::test]
    async fn an_enrolled_repository_without_a_projection_remains_visible_in_fleet() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
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
        let mut enrollment = EnrollmentStore::connect(&database_url)
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
        let challenge = enrollment
            .issue_challenge(&request, &[3; 32], now)
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
}
