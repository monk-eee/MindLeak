//! ADR-0145 decision 4-8: production recovery-execution run. Split from
//! `recovery_execution.rs` (preview/confirm/status) for the same
//! module-length reason Purge/Snapshot/Export/Recovery are already their own
//! files -- this file owns only the one route that actually mutates the
//! authoritative database.

use std::time::SystemTime;

use ackplane_server::administration_store::{RecoveryExecutionOutcome, RecoveryExecutionReceipt};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;

use super::recovery_execution::{hex_encode, unix_seconds};
use super::{administration_error_status, AdministrationApiState};

#[derive(Serialize)]
pub(super) struct RecoveryExecutionReceiptResponse {
    receipt_id: String,
    outcome: &'static str,
    reason: String,
    old_manifest_digest_hex: String,
    new_manifest_digest_hex: String,
    rehearsal_id: String,
    previewing_node_id: String,
    previewing_public_key_fingerprint: String,
    confirming_node_id: String,
    confirming_public_key_fingerprint: String,
    occurred_at_seconds: Option<u64>,
}

impl From<RecoveryExecutionReceipt> for RecoveryExecutionReceiptResponse {
    fn from(receipt: RecoveryExecutionReceipt) -> Self {
        Self {
            receipt_id: receipt.receipt_id,
            outcome: recovery_execution_outcome_label(receipt.outcome),
            reason: receipt.reason,
            old_manifest_digest_hex: hex_encode(&receipt.old_manifest_digest),
            new_manifest_digest_hex: hex_encode(&receipt.new_manifest_digest),
            rehearsal_id: receipt.rehearsal_id,
            previewing_node_id: receipt.previewing_node_id,
            previewing_public_key_fingerprint: receipt.previewing_public_key_fingerprint,
            confirming_node_id: receipt.confirming_node_id,
            confirming_public_key_fingerprint: receipt.confirming_public_key_fingerprint,
            occurred_at_seconds: unix_seconds(receipt.occurred_at),
        }
    }
}

/// Runs (or refuses, or records a genuine `pg_restore` failure for) a
/// previously previewed *and confirmed* recovery execution -- the one route
/// that actually mutates the authoritative database (ADR-0145 decision 4-8).
/// Deliberately a distinct step from `confirm_recovery_execution`, not fused
/// into it: the dual-signing-key preview/confirm pair is the request's full
/// authorization (decision 4), so this route needs no signature of its own --
/// it only consumes an authorization that already exists, re-checking
/// single-tenant attestation and rehearsal freshness at the moment it runs
/// (decision 3, decision 6). Idempotent: replays of an already-executed
/// request return the same durable receipt without ever running `pg_restore`
/// twice.
pub(super) async fn execute_recovery_execution(
    State(state): State<AdministrationApiState>,
    Path(request_id): Path<String>,
) -> Result<Json<RecoveryExecutionReceiptResponse>, StatusCode> {
    let Some(snapshot_config) = state.snapshot.clone() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let now = SystemTime::now();
    // Disclosure stays tenant-bounded, exactly like preview/confirm/status,
    // even though the operation itself is always platform-scoped.
    let administration = &state.administration;
    let request = administration
        .recovery_execution_request(&request_id)
        .await
        .map_err(administration_error_status)?
        .ok_or(StatusCode::NOT_FOUND)?;
    if request.tenant_id != state.tenant_id.as_ref() {
        return Err(StatusCode::NOT_FOUND);
    }
    let receipt = administration
        .execute_recovery(&request_id, &snapshot_config, now)
        .await
        .map_err(administration_error_status)?;
    Ok(Json(receipt.into()))
}

fn recovery_execution_outcome_label(outcome: RecoveryExecutionOutcome) -> &'static str {
    match outcome {
        RecoveryExecutionOutcome::Succeeded => "succeeded",
        RecoveryExecutionOutcome::Failed => "failed",
        RecoveryExecutionOutcome::Refused => "refused",
    }
}
