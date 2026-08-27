//! Transactional directive enqueueing and immutable receipt recording.

use std::time::SystemTime;

use ackplane_protocol::{supervisor::SupervisorCapabilities, v1};
use prost::Message;
use tokio_postgres::Transaction;

use super::{
    model::{
        directive_from_row, directive_request_digest, format_timestamp, normalize_timestamp,
        receipt_digest, receipt_from_row, validate_directive, validate_receipt,
    },
    DirectiveReceiptOutcome, DirectiveStore, DirectiveStoreError, DirectiveWriteOutcome,
};

impl DirectiveStore {
    /// Records one immutable directive and assigns its per-target sequence.
    pub async fn enqueue(
        &mut self,
        directive: v1::AgentDirective,
    ) -> Result<DirectiveWriteOutcome, DirectiveStoreError> {
        let now = normalize_timestamp(SystemTime::now());
        let transaction = self.client.transaction().await?;
        let outcome = enqueue_in_transaction(&transaction, directive, now).await?;
        transaction.commit().await?;
        Ok(outcome)
    }

    /// Appends one receipt only when it binds to an existing directive exactly.
    pub async fn record_receipt(
        &mut self,
        receipt: v1::DirectiveReceipt,
    ) -> Result<DirectiveReceiptOutcome, DirectiveStoreError> {
        let transaction = self.client.transaction().await?;
        let row = transaction
            .query_opt(
                "SELECT directive_payload, request_digest, recorded_at, created_at \
                 FROM agent_directives WHERE tenant_id = $1 AND repository_id = $2 AND directive_id = $3 \
                 FOR KEY SHARE",
                &[&receipt.tenant_id, &receipt.repository_id, &receipt.directive_id],
            )
            .await?
            .ok_or(DirectiveStoreError::UnknownDirective)?;
        let directive = directive_from_row(&row)?;
        let created_at: SystemTime = row.get("created_at");
        let occurred_at = validate_receipt(&receipt, &directive.directive, created_at)?;
        let digest = receipt_digest(&receipt);
        let payload = receipt.encode_to_vec();
        let status = i16::try_from(receipt.status)
            .map_err(|_| DirectiveStoreError::InvalidReceiptOutcome)?;
        let reason = i16::try_from(receipt.reason)
            .map_err(|_| DirectiveStoreError::InvalidReceiptOutcome)?;

        let inserted = transaction
            .query_opt(
                "INSERT INTO directive_receipts (tenant_id, repository_id, directive_id, node_id, \
                     agent_session_id, directive_sequence, receipt_status, receipt_reason, occurred_at, \
                     payload_digest, receipt_digest, checkpoint_refs, evidence_refs, diagnostic, receipt_payload) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) \
                 ON CONFLICT (tenant_id, repository_id, directive_id, receipt_digest) DO NOTHING \
                 RETURNING receipt_position, receipt_payload, receipt_digest, recorded_at",
                &[
                    &receipt.tenant_id,
                    &receipt.repository_id,
                    &receipt.directive_id,
                    &receipt.node_id,
                    &receipt.agent_session_id,
                    &i64::try_from(receipt.directive_sequence)
                        .map_err(|_| DirectiveStoreError::ReceiptMismatch)?,
                    &status,
                    &reason,
                    &occurred_at,
                    &receipt.payload_digest,
                    &digest,
                    &receipt.checkpoint_refs,
                    &receipt.evidence_refs,
                    &receipt.diagnostic,
                    &payload,
                ],
            )
            .await?;
        let (record, idempotent_replay) = match inserted {
            Some(row) => (receipt_from_row(&row)?, false),
            None => {
                let row = transaction
                    .query_one(
                        "SELECT receipt_position, receipt_payload, receipt_digest, recorded_at \
                         FROM directive_receipts WHERE tenant_id = $1 AND repository_id = $2 \
                           AND directive_id = $3 AND receipt_digest = $4 FOR KEY SHARE",
                        &[
                            &receipt.tenant_id,
                            &receipt.repository_id,
                            &receipt.directive_id,
                            &digest,
                        ],
                    )
                    .await?;
                (receipt_from_row(&row)?, true)
            }
        };
        transaction.commit().await?;
        Ok(DirectiveReceiptOutcome {
            record,
            idempotent_replay,
        })
    }
}

/// The `enqueue` body, callable against a transaction another store already
/// holds open. Cross-table atomicity (a Work command and its directive
/// committing or rolling back together) is only possible on one connection;
/// see `work_command_store::execute`'s own doc comment for why that module
/// calls this directly instead of a second `DirectiveStore` connection.
pub(crate) async fn enqueue_in_transaction(
    transaction: &Transaction<'_>,
    mut directive: v1::AgentDirective,
    now: SystemTime,
) -> Result<DirectiveWriteOutcome, DirectiveStoreError> {
    let (requirement, expires_at) = validate_directive(&directive, now)?;
    let request_digest = directive_request_digest(&directive);
    let capabilities = target_capabilities(transaction, &directive).await?;
    if !capabilities
        .supported_directives
        .contains(&requirement.capability)
    {
        return Err(DirectiveStoreError::CapabilityMissing);
    }

    if let Some(record) = existing_by_directive_id(transaction, &directive).await? {
        return replay_or_conflict(record, &request_digest);
    }
    if let Some(record) = existing_by_idempotency_key(transaction, &directive).await? {
        return replay_or_conflict(record, &request_digest);
    }

    let current_position = lock_stream(transaction, &directive).await?;
    let sequence = current_position
        .checked_add(1)
        .filter(|position| *position > 0)
        .ok_or(DirectiveStoreError::SequenceExhausted)?;
    directive.sequence =
        u64::try_from(sequence).map_err(|_| DirectiveStoreError::SequenceExhausted)?;
    directive.created_at = format_timestamp(now)?;
    let payload = directive.encode_to_vec();
    let kind =
        i16::try_from(directive.kind).map_err(|_| DirectiveStoreError::UnsupportedDirective)?;
    let task_id = optional(&directive.task_id);
    let goal_id = optional(&directive.goal_id);
    let context_packet_id = optional(&directive.context_packet_id);

    let inserted = transaction
        .query_opt(
            "INSERT INTO agent_directives (tenant_id, repository_id, directive_id, node_id, \
                 agent_session_id, project_id, directive_kind, schema_version, issuing_principal_id, \
                 rationale, task_id, goal_id, context_packet_id, created_at, expires_at, \
                 directive_sequence, idempotency_key, request_digest, payload_digest, \
                 required_capability, policy_refs, knowledge_refs, evidence_refs, directive_payload) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24) \
             ON CONFLICT DO NOTHING \
             RETURNING directive_payload, request_digest, recorded_at",
            &[
                &directive.tenant_id,
                &directive.repository_id,
                &directive.directive_id,
                &directive.target_node_id,
                &directive.target_agent_session_id,
                &directive.project_id,
                &kind,
                &directive.schema_version,
                &directive.issuing_principal_id,
                &directive.rationale,
                &task_id,
                &goal_id,
                &context_packet_id,
                &now,
                &expires_at,
                &sequence,
                &directive.idempotency_key,
                &request_digest,
                &directive.payload_digest,
                &directive.required_capability,
                &directive.policy_refs,
                &directive.knowledge_refs,
                &directive.evidence_refs,
                &payload,
            ],
        )
        .await?;
    let Some(row) = inserted else {
        let record = existing_by_directive_id(transaction, &directive)
            .await?
            .or(existing_by_idempotency_key(transaction, &directive).await?)
            .ok_or(DirectiveStoreError::IdempotencyConflict)?;
        return replay_or_conflict(record, &request_digest);
    };
    transaction
        .execute(
            "UPDATE directive_stream_heads SET stream_position = $5 \
             WHERE tenant_id = $1 AND repository_id = $2 AND node_id = $3 AND agent_session_id = $4",
            &[
                &directive.tenant_id,
                &directive.repository_id,
                &directive.target_node_id,
                &directive.target_agent_session_id,
                &sequence,
            ],
        )
        .await?;
    let record = directive_from_row(&row)?;
    Ok(DirectiveWriteOutcome {
        record,
        idempotent_replay: false,
    })
}

async fn target_capabilities(
    transaction: &Transaction<'_>,
    directive: &v1::AgentDirective,
) -> Result<SupervisorCapabilities, DirectiveStoreError> {
    let row = transaction
        .query_opt(
            "SELECT registrations.node_id, registrations.capabilities \
             FROM supervisor_sessions AS sessions \
             JOIN supervisor_registrations AS registrations \
               ON registrations.tenant_id = sessions.tenant_id \
              AND registrations.repository_id = sessions.repository_id \
              AND registrations.supervisor_id = sessions.supervisor_id \
             WHERE sessions.tenant_id = $1 AND sessions.repository_id = $2 AND sessions.session_id = $3 \
             FOR KEY SHARE",
            &[
                &directive.tenant_id,
                &directive.repository_id,
                &directive.target_agent_session_id,
            ],
        )
        .await?
        .ok_or(DirectiveStoreError::UnknownSupervisorSession)?;
    let node_id: String = row.get("node_id");
    if node_id != directive.target_node_id {
        return Err(DirectiveStoreError::TargetNodeMismatch);
    }
    let capabilities: SupervisorCapabilities = serde_json::from_str(row.get("capabilities"))?;
    capabilities.validate()?;
    Ok(capabilities)
}

async fn lock_stream(
    transaction: &Transaction<'_>,
    directive: &v1::AgentDirective,
) -> Result<i64, DirectiveStoreError> {
    Ok(transaction
        .query_one(
            "INSERT INTO directive_stream_heads (tenant_id, repository_id, node_id, agent_session_id, stream_position) \
             VALUES ($1,$2,$3,$4,0) \
             ON CONFLICT (tenant_id, repository_id, node_id, agent_session_id) DO UPDATE \
                 SET stream_position = directive_stream_heads.stream_position \
             RETURNING stream_position",
            &[
                &directive.tenant_id,
                &directive.repository_id,
                &directive.target_node_id,
                &directive.target_agent_session_id,
            ],
        )
        .await?
        .get("stream_position"))
}

async fn existing_by_directive_id(
    transaction: &Transaction<'_>,
    directive: &v1::AgentDirective,
) -> Result<Option<super::DirectiveRecord>, DirectiveStoreError> {
    transaction
        .query_opt(
            "SELECT directive_payload, request_digest, recorded_at FROM agent_directives \
             WHERE tenant_id = $1 AND repository_id = $2 AND directive_id = $3 FOR KEY SHARE",
            &[
                &directive.tenant_id,
                &directive.repository_id,
                &directive.directive_id,
            ],
        )
        .await?
        .map(|row| directive_from_row(&row))
        .transpose()
}

async fn existing_by_idempotency_key(
    transaction: &Transaction<'_>,
    directive: &v1::AgentDirective,
) -> Result<Option<super::DirectiveRecord>, DirectiveStoreError> {
    transaction
        .query_opt(
            "SELECT directive_payload, request_digest, recorded_at FROM agent_directives \
             WHERE tenant_id = $1 AND repository_id = $2 AND node_id = $3 \
               AND agent_session_id = $4 AND idempotency_key = $5 FOR KEY SHARE",
            &[
                &directive.tenant_id,
                &directive.repository_id,
                &directive.target_node_id,
                &directive.target_agent_session_id,
                &directive.idempotency_key,
            ],
        )
        .await?
        .map(|row| directive_from_row(&row))
        .transpose()
}

fn replay_or_conflict(
    record: super::DirectiveRecord,
    request_digest: &[u8],
) -> Result<DirectiveWriteOutcome, DirectiveStoreError> {
    if record.request_digest != request_digest {
        return Err(DirectiveStoreError::IdempotencyConflict);
    }
    Ok(DirectiveWriteOutcome {
        record,
        idempotent_replay: true,
    })
}

fn optional(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}
