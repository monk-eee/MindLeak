//! Durable, bounded human decision (escalation) requests and their
//! append-only lifecycle events (ADR-0115 item 5: escalation is a
//! first-class durable state).
//!
//! This store is intentionally server-internal: a future policy/escalation
//! routing layer decides when to create a request and which verified human
//! principal may resolve it. It does not expose a browser command itself and
//! does not treat a session label, model score, or agent assertion as a
//! decision (ADR-0115 items 1 and 8).

use std::time::SystemTime;

use crate::db_pool::{PgConnection, PgPool};
use getrandom::getrandom;

const MIGRATION: &str = include_str!("../migrations/0054_human_decision_requests.sql");
pub(super) const PROJECTION_COLUMNS: &str =
    "decision_id, proposing_principal_id, proposed_action, target, reason, \
    context_packet_digest, evidence_digest, alternatives, safe_behavior, related_delegation_id, \
    requested_at, expires_at, status, version, source_event_position, resolved_at, \
    resolved_by_principal_id, resolution_rationale";
pub(super) const EVENT_COLUMNS: &str =
    "decision_id, stream_position, event_kind, actor_principal_id, \
    proposed_action, target, reason, context_packet_digest, evidence_digest, alternatives, \
    safe_behavior, related_delegation_id, expires_at, rationale, expected_prior_version, \
    resulting_version, idempotency_key, payload_digest, schema_version, recorded_at";

mod model;
mod read;
mod replay;

pub use model::{
    HumanDecisionEvent, HumanDecisionEventKind, HumanDecisionEventPayload, HumanDecisionOutcome,
    HumanDecisionProjection, HumanDecisionRequest, HumanDecisionRequestedPayload,
    HumanDecisionResolutionOutcome, HumanDecisionResolutionRequest, HumanDecisionStatus,
    HumanDecisionStoreError, SafeBehavior,
};
pub use read::{HumanDecisionListCursor, HumanDecisionListPage};

use model::{
    event_schema_version, normalize_timestamp, projection_at_event, request_payload_digest,
    resolution_payload_digest, row_to_event, validate_request, validate_resolution,
};
use replay::{
    advance_stream, idempotent_outcome, lock_stream, next_stream_position,
    projection_from_transaction, requested_event_from_transaction,
};

/// Authoritative persistence for the ADR-0115 human decision namespace.
pub struct HumanDecisionStore {
    pool: PgPool,
}

impl HumanDecisionStore {
    /// Takes a clone of the process's single pool (ADR-0143 decision 1), not a
    /// database URL: a store that resolved its own connection would be exactly
    /// the per-store demand the pool exists to bound.
    pub async fn connect(pool: &PgPool) -> Result<Self, HumanDecisionStoreError> {
        let mut connection = pool.get().await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::HUMAN_DECISION_REQUESTS,
            MIGRATION,
        )
        .await?;
        Ok(Self { pool: pool.clone() })
    }

    /// One checked-out connection, held only for the call that asked for it.
    ///
    /// A caller that opens a transaction keeps this binding alive for the life
    /// of that transaction, which is the one case where holding a pooled
    /// connection across `.await` points is correct rather than accidental.
    async fn connection(&self) -> Result<PgConnection, HumanDecisionStoreError> {
        Ok(crate::db_pool::checkout(&self.pool).await?)
    }

    /// Records a durable escalation request after the caller has already
    /// verified its proposing principal (ADR-0115 item 5).
    pub async fn request(
        &self,
        mut request: HumanDecisionRequest,
    ) -> Result<HumanDecisionOutcome, HumanDecisionStoreError> {
        request.expires_at = normalize_timestamp(request.expires_at);
        let now = normalize_timestamp(SystemTime::now());
        validate_request(&request, now)?;
        let payload_digest = request_payload_digest(&request);

        // One connection, checked out once and held until commit (ADR-0143
        // decision 4): the stream lock below must stay on a stable connection.
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        let current_position =
            lock_stream(&transaction, &request.tenant_id, &request.repository_id).await?;
        if let Some(outcome) = idempotent_outcome(
            &transaction,
            &request.tenant_id,
            &request.repository_id,
            &request.idempotency_key,
            model::HumanDecisionEventKind::Requested,
            &payload_digest,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(outcome);
        }

        let decision_id = unique_decision_id()?;
        let event_position = next_stream_position(current_position)?;
        let safe_behavior_code = request.safe_behavior.as_i16();
        let event_row = transaction
            .query_one(
                &format!(
                    "INSERT INTO human_decision_events (tenant_id, repository_id, stream_position, \
                         decision_id, event_kind, actor_principal_id, proposed_action, target, reason, \
                         context_packet_digest, evidence_digest, alternatives, safe_behavior, \
                         related_delegation_id, expires_at, expected_prior_version, resulting_version, \
                         idempotency_key, payload_digest, schema_version) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20) \
                     RETURNING {EVENT_COLUMNS}"
                ),
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &event_position,
                    &decision_id,
                    &model::HumanDecisionEventKind::Requested.as_i16(),
                    &request.verified_proposing_principal_id,
                    &request.proposed_action,
                    &request.target,
                    &request.reason,
                    &request.context_packet_digest,
                    &request.evidence_digest,
                    &request.alternatives,
                    &safe_behavior_code,
                    &request.related_delegation_id,
                    &request.expires_at,
                    &0_i32,
                    &1_i32,
                    &request.idempotency_key,
                    &payload_digest,
                    &event_schema_version(),
                ],
            )
            .await?;
        let event = row_to_event(&event_row)?;
        transaction
            .execute(
                "INSERT INTO human_decision_projections (tenant_id, repository_id, decision_id, \
                     proposing_principal_id, proposed_action, target, reason, context_packet_digest, \
                     evidence_digest, alternatives, safe_behavior, related_delegation_id, requested_at, \
                     expires_at, status, version, source_event_position) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &decision_id,
                    &request.verified_proposing_principal_id,
                    &request.proposed_action,
                    &request.target,
                    &request.reason,
                    &request.context_packet_digest,
                    &request.evidence_digest,
                    &request.alternatives,
                    &safe_behavior_code,
                    &request.related_delegation_id,
                    &event.recorded_at,
                    &request.expires_at,
                    &model::HumanDecisionStatus::Pending.as_i16(),
                    &1_i32,
                    &event_position,
                ],
            )
            .await?;
        advance_stream(
            &transaction,
            &request.tenant_id,
            &request.repository_id,
            event_position,
        )
        .await?;
        let projection = projection_at_event(&event, &event)?;
        transaction.commit().await?;
        Ok(HumanDecisionOutcome {
            projection,
            event,
            idempotent_replay: false,
        })
    }

    /// Records an approval or denial under a caller-verified resolving
    /// identity, refusing a resolver who also proposed the request
    /// (ADR-0115 item 8: separation of duties).
    pub async fn resolve(
        &self,
        request: HumanDecisionResolutionRequest,
    ) -> Result<HumanDecisionOutcome, HumanDecisionStoreError> {
        validate_resolution(&request)?;
        let payload_digest = resolution_payload_digest(&request);
        let event_kind = match request.outcome {
            HumanDecisionResolutionOutcome::Approved => model::HumanDecisionEventKind::Approved,
            HumanDecisionResolutionOutcome::Denied => model::HumanDecisionEventKind::Denied,
        };

        // One connection, checked out once and held until commit (ADR-0143
        // decision 4): the stream lock below must stay on a stable connection.
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        let current_position =
            lock_stream(&transaction, &request.tenant_id, &request.repository_id).await?;
        if let Some(outcome) = idempotent_outcome(
            &transaction,
            &request.tenant_id,
            &request.repository_id,
            &request.idempotency_key,
            event_kind,
            &payload_digest,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(outcome);
        }

        let current_projection = projection_from_transaction(
            &transaction,
            &request.tenant_id,
            &request.repository_id,
            &request.decision_id,
            true,
        )
        .await?
        .ok_or(HumanDecisionStoreError::NotFound)?;
        if current_projection.status != model::HumanDecisionStatus::Pending {
            return Err(HumanDecisionStoreError::AlreadyResolved);
        }
        if current_projection.version != request.expected_version {
            return Err(HumanDecisionStoreError::VersionConflict);
        }
        if current_projection.proposing_principal_id == request.verified_resolving_principal_id {
            return Err(HumanDecisionStoreError::SeparationOfDutiesViolation);
        }

        let event_position = next_stream_position(current_position)?;
        let resulting_version = request
            .expected_version
            .checked_add(1)
            .ok_or(HumanDecisionStoreError::InvalidVersion)?;
        let event_row = transaction
            .query_one(
                &format!(
                    "INSERT INTO human_decision_events (tenant_id, repository_id, stream_position, \
                         decision_id, event_kind, actor_principal_id, rationale, expected_prior_version, \
                         resulting_version, idempotency_key, payload_digest, schema_version) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) \
                     RETURNING {EVENT_COLUMNS}"
                ),
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &event_position,
                    &request.decision_id,
                    &event_kind.as_i16(),
                    &request.verified_resolving_principal_id,
                    &request.rationale,
                    &i32::try_from(request.expected_version)
                        .expect("validated expected version"),
                    &i32::try_from(resulting_version)
                        .expect("human decision version fits PostgreSQL integer"),
                    &request.idempotency_key,
                    &payload_digest,
                    &event_schema_version(),
                ],
            )
            .await?;
        let event = row_to_event(&event_row)?;
        let status = match request.outcome {
            HumanDecisionResolutionOutcome::Approved => model::HumanDecisionStatus::Approved,
            HumanDecisionResolutionOutcome::Denied => model::HumanDecisionStatus::Denied,
        };
        transaction
            .execute(
                "UPDATE human_decision_projections SET status = $4, version = $5, \
                     source_event_position = $6, resolved_at = $7, resolved_by_principal_id = $8, \
                     resolution_rationale = $9 \
                 WHERE tenant_id = $1 AND repository_id = $2 AND decision_id = $3",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.decision_id,
                    &status.as_i16(),
                    &i32::try_from(resulting_version)
                        .expect("human decision version fits PostgreSQL integer"),
                    &event_position,
                    &event.recorded_at,
                    &request.verified_resolving_principal_id,
                    &request.rationale,
                ],
            )
            .await?;
        advance_stream(
            &transaction,
            &request.tenant_id,
            &request.repository_id,
            event_position,
        )
        .await?;
        let requested_event = requested_event_from_transaction(
            &transaction,
            &request.tenant_id,
            &request.repository_id,
            &request.decision_id,
        )
        .await?;
        let projection = projection_at_event(&requested_event, &event)?;
        transaction.commit().await?;
        Ok(HumanDecisionOutcome {
            projection,
            event,
            idempotent_replay: false,
        })
    }
}

fn unique_decision_id() -> Result<String, HumanDecisionStoreError> {
    let mut bytes = [0_u8; 16];
    getrandom(&mut bytes)?;
    let suffix: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!("decision-{suffix}"))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn unique_scope() -> (String, String) {
        let mut bytes = [0_u8; 8];
        getrandom(&mut bytes).expect("the OS random source should be available");
        let suffix: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        (
            format!("tenant-human-decision-{suffix}"),
            format!("repository-human-decision-{suffix}"),
        )
    }

    fn decision_request(
        tenant_id: String,
        repository_id: String,
        idempotency_key: &str,
    ) -> HumanDecisionRequest {
        HumanDecisionRequest {
            tenant_id,
            repository_id,
            verified_proposing_principal_id: "principal:agent-integration".to_string(),
            proposed_action: "action:export-sensitive-data".to_string(),
            target: "artifact:integration".to_string(),
            reason: "exceeds routine delegation budget".to_string(),
            context_packet_digest: vec![1; 32],
            evidence_digest: vec![2; 32],
            alternatives: "narrow the export or wait for a broader delegation".to_string(),
            safe_behavior: SafeBehavior::CheckpointAndPause,
            related_delegation_id: Some("delegation-integration".to_string()),
            expires_at: SystemTime::now() + Duration::from_secs(600),
            idempotency_key: idempotency_key.to_string(),
        }
    }

    #[tokio::test]
    async fn requesting_then_resolving_by_a_different_principal_succeeds() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let store = HumanDecisionStore::connect(&pool)
            .await
            .expect("store should connect");
        let (tenant_id, repository_id) = unique_scope();
        let request_outcome = store
            .request(decision_request(
                tenant_id.clone(),
                repository_id.clone(),
                "human-decision:request:1",
            ))
            .await
            .expect("request should succeed");
        assert_eq!(
            request_outcome.projection.status,
            HumanDecisionStatus::Pending
        );

        let resolve_outcome = store
            .resolve(HumanDecisionResolutionRequest {
                tenant_id,
                repository_id,
                decision_id: request_outcome.projection.decision_id.clone(),
                verified_resolving_principal_id: "principal:human-reviewer".to_string(),
                outcome: HumanDecisionResolutionOutcome::Approved,
                rationale: "reviewed the context packet and evidence".to_string(),
                expected_version: request_outcome.projection.version,
                idempotency_key: "human-decision:resolve:1".to_string(),
            })
            .await
            .expect("resolve should succeed");
        assert_eq!(
            resolve_outcome.projection.status,
            HumanDecisionStatus::Approved
        );
        assert_eq!(
            resolve_outcome
                .projection
                .resolved_by_principal_id
                .as_deref(),
            Some("principal:human-reviewer")
        );
    }

    #[tokio::test]
    async fn resolving_by_the_proposing_principal_is_refused() {
        // Regression guard for ADR-0115 item 8: an agent that proposed an
        // escalation must never be able to approve or deny its own request.
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let store = HumanDecisionStore::connect(&pool)
            .await
            .expect("store should connect");
        let (tenant_id, repository_id) = unique_scope();
        let request_outcome = store
            .request(decision_request(
                tenant_id.clone(),
                repository_id.clone(),
                "human-decision:request:2",
            ))
            .await
            .expect("request should succeed");

        let error = store
            .resolve(HumanDecisionResolutionRequest {
                tenant_id,
                repository_id,
                decision_id: request_outcome.projection.decision_id.clone(),
                verified_resolving_principal_id: "principal:agent-integration".to_string(),
                outcome: HumanDecisionResolutionOutcome::Approved,
                rationale: "self-approval attempt".to_string(),
                expected_version: request_outcome.projection.version,
                idempotency_key: "human-decision:resolve:2".to_string(),
            })
            .await
            .expect_err("a proposing principal must not resolve its own request");
        assert!(matches!(
            error,
            HumanDecisionStoreError::SeparationOfDutiesViolation
        ));
    }

    #[tokio::test]
    async fn resolving_an_already_resolved_request_is_refused() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let store = HumanDecisionStore::connect(&pool)
            .await
            .expect("store should connect");
        let (tenant_id, repository_id) = unique_scope();
        let request_outcome = store
            .request(decision_request(
                tenant_id.clone(),
                repository_id.clone(),
                "human-decision:request:3",
            ))
            .await
            .expect("request should succeed");
        store
            .resolve(HumanDecisionResolutionRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                decision_id: request_outcome.projection.decision_id.clone(),
                verified_resolving_principal_id: "principal:human-reviewer".to_string(),
                outcome: HumanDecisionResolutionOutcome::Denied,
                rationale: "insufficient evidence".to_string(),
                expected_version: request_outcome.projection.version,
                idempotency_key: "human-decision:resolve:3a".to_string(),
            })
            .await
            .expect("first resolution should succeed");

        let error = store
            .resolve(HumanDecisionResolutionRequest {
                tenant_id,
                repository_id,
                decision_id: request_outcome.projection.decision_id.clone(),
                verified_resolving_principal_id: "principal:human-reviewer-2".to_string(),
                outcome: HumanDecisionResolutionOutcome::Approved,
                rationale: "second look".to_string(),
                expected_version: request_outcome.projection.version + 1,
                idempotency_key: "human-decision:resolve:3b".to_string(),
            })
            .await
            .expect_err("an already-resolved request must refuse a second resolution");
        assert!(matches!(error, HumanDecisionStoreError::AlreadyResolved));
    }

    #[tokio::test]
    async fn requesting_with_a_past_expiry_is_rejected() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let store = HumanDecisionStore::connect(&pool)
            .await
            .expect("store should connect");
        let (tenant_id, repository_id) = unique_scope();
        let mut request = decision_request(tenant_id, repository_id, "human-decision:request:4");
        request.expires_at = SystemTime::now() - Duration::from_secs(1);

        let error = store
            .request(request)
            .await
            .expect_err("a request expiring in the past must be rejected");
        assert!(matches!(error, HumanDecisionStoreError::InvalidTimeWindow));
    }

    /// The read surface is what a human queue is built on, so its tenant
    /// scoping is load-bearing: one tenant must never list or fetch another
    /// tenant's escalations even when it knows the repository and decision id.
    #[tokio::test]
    async fn reads_stay_inside_their_tenant_and_can_narrow_to_one_status() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let store = HumanDecisionStore::connect(&pool)
            .await
            .expect("store should connect");
        let (tenant_id, repository_id) = unique_scope();
        let waiting = store
            .request(decision_request(
                tenant_id.clone(),
                repository_id.clone(),
                "human-decision:request:read-waiting",
            ))
            .await
            .expect("request should succeed");
        let resolved = store
            .request(decision_request(
                tenant_id.clone(),
                repository_id.clone(),
                "human-decision:request:read-resolved",
            ))
            .await
            .expect("request should succeed");
        store
            .resolve(HumanDecisionResolutionRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                decision_id: resolved.projection.decision_id.clone(),
                verified_resolving_principal_id: "principal:human-reviewer".to_string(),
                outcome: HumanDecisionResolutionOutcome::Approved,
                rationale: "inside the approved envelope".to_string(),
                expected_version: resolved.projection.version,
                idempotency_key: "human-decision:resolve:read-resolved".to_string(),
            })
            .await
            .expect("resolve should succeed");

        let all = store
            .list_page(&tenant_id, &repository_id, None, None, 10)
            .await
            .expect("list should succeed");
        assert_eq!(all.entries.len(), 2);
        assert!(all.next_after.is_none());

        let pending = store
            .list_page(
                &tenant_id,
                &repository_id,
                Some(HumanDecisionStatus::Pending),
                None,
                10,
            )
            .await
            .expect("filtered list should succeed");
        assert_eq!(pending.entries.len(), 1);
        assert_eq!(
            pending.entries[0].decision_id,
            waiting.projection.decision_id
        );

        let other_tenant = format!("{tenant_id}-other");
        let leaked = store
            .list_page(&other_tenant, &repository_id, None, None, 10)
            .await
            .expect("list should succeed");
        assert!(
            leaked.entries.is_empty(),
            "another tenant must not list these escalations"
        );
        let leaked = store
            .get(
                &other_tenant,
                &repository_id,
                &waiting.projection.decision_id,
            )
            .await
            .expect("get should succeed");
        assert!(
            leaked.is_none(),
            "another tenant must not fetch this escalation by id"
        );

        let found = store
            .get(&tenant_id, &repository_id, &waiting.projection.decision_id)
            .await
            .expect("get should succeed")
            .expect("the owning tenant sees its own escalation");
        assert_eq!(found.status, HumanDecisionStatus::Pending);
    }
}
