//! ADR-0145 decision 4-5: production recovery-execution preview/confirmation,
//! reusing ADR-0134's dual-signing-key Lifecycle-purge pattern verbatim,
//! scoped to Recovery (`RecoveryExecutionOperation`, distinct domain
//! separator). Always platform-scoped, per decision 6 -- there is no
//! per-repository path segment for these routes, unlike Lifecycle purge.
//! Confirming here never runs `pg_restore`; it only records that a second,
//! distinct enrolled key authorized the request. Production execution
//! (decision 4-8, `execute_recovery_execution`) is a distinct, later route
//! that actually consumes that authorization and runs the real restore --
//! split into `recovery_execution_run.rs` for the same module-length reason
//! Purge/Snapshot/Export/Recovery are already their own files.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ackplane_protocol::{
    purge_confirmation_auth::{recovery_execution_signing_bytes, RecoveryExecutionOperation},
    v1,
};
use ackplane_server::administration_store::{
    RecoveryConfirmation, RecoveryConfirmationOutcome, RecoveryExecutionPreviewRequest,
    RecoveryExecutionRequest,
};
use ackplane_server::claim_signature::{self, ClaimAuthRefusal};
use ackplane_server::signing_keys::{EnvelopeBinding, KeyResolution};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use super::snapshot::execute_and_record_snapshot;
use super::{administration_error_status, AdministrationApiState};

/// An enrolled node's operation-bound proof for one Recovery-execution
/// request, mirroring `purge::SignedPurgeAuthentication` exactly (ADR-0145
/// decision 4 reuses ADR-0134's shape verbatim).
#[derive(Deserialize)]
pub(super) struct SignedRecoveryAuthentication {
    signing_key_id: String,
    node_id: String,
    signed_at: String,
    nonce: Vec<u8>,
    signature: Vec<u8>,
}

impl SignedRecoveryAuthentication {
    fn into_claim_authentication(self) -> v1::ClaimAuthentication {
        v1::ClaimAuthentication {
            signing_key_id: self.signing_key_id,
            node_id: self.node_id,
            signed_at: self.signed_at,
            nonce: self.nonce,
            signature: self.signature,
        }
    }
}

struct RecoveryPrincipal {
    signing_key_id: String,
    node_id: String,
    public_key_fingerprint: String,
}

#[derive(Deserialize)]
pub(super) struct PreviewRecoveryExecutionRequest {
    policy_id: String,
    /// Authorizes the safety Snapshot decision 5 requires triggering as part
    /// of preview construction -- a distinct, already-adopted Snapshot
    /// policy, never the Recovery-execution policy itself (Snapshot and
    /// Recovery execution are different `AdministrationOperation` variants).
    snapshot_policy_id: String,
    /// The enrolled key's own repository, needed to resolve its signing key
    /// (`EnvelopeBinding::repository_id`). The recorded scope is always
    /// `AdministrationScope::Platform` regardless of this value -- decision 6
    /// forbids the request from claiming a narrower blast radius than the
    /// artifact actually has.
    repository_id: String,
    artifact_request_id: String,
    manifest_digest_hex: String,
    /// A fresh idempotency key for the safety Snapshot's own request, kept
    /// distinct from this preview's own `idempotency_key` -- the two are
    /// different durable records with different identities.
    safety_snapshot_idempotency_key: String,
    rehearsal_id: String,
    confirmation_window_seconds: u64,
    idempotency_key: String,
    authentication: SignedRecoveryAuthentication,
}

#[derive(Serialize)]
pub(super) struct RecoveryExecutionPreviewResponse {
    request_id: String,
    artifact_request_id: String,
    manifest_digest_hex: String,
    safety_snapshot_receipt_id: String,
    safety_snapshot_digest_hex: String,
    rehearsal_id: String,
    confirmation_expires_at_seconds: Option<u64>,
    idempotent_replay: bool,
}

impl From<RecoveryExecutionRequest> for RecoveryExecutionPreviewResponse {
    fn from(request: RecoveryExecutionRequest) -> Self {
        Self {
            request_id: request.request_id,
            artifact_request_id: request.artifact_request_id,
            manifest_digest_hex: hex_encode(&request.manifest_digest),
            safety_snapshot_receipt_id: request.safety_snapshot_receipt_id,
            safety_snapshot_digest_hex: hex_encode(&request.safety_snapshot_digest),
            rehearsal_id: request.rehearsal_id,
            confirmation_expires_at_seconds: unix_seconds(request.confirmation_expires_at),
            idempotent_replay: false,
        }
    }
}

/// Triggers a fresh platform Snapshot (decision 5's "one before" safety
/// point; its failure fails this preview outright, before any recovery
/// request row exists) and, once it succeeds, previews the recovery
/// execution against the named artifact, digest, and rehearsal.
pub(super) async fn preview_recovery_execution(
    State(state): State<AdministrationApiState>,
    Json(request): Json<PreviewRecoveryExecutionRequest>,
) -> Result<Json<RecoveryExecutionPreviewResponse>, StatusCode> {
    let Some(snapshot_config) = state.snapshot.clone() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let PreviewRecoveryExecutionRequest {
        policy_id,
        snapshot_policy_id,
        repository_id,
        artifact_request_id,
        manifest_digest_hex,
        safety_snapshot_idempotency_key,
        rehearsal_id,
        confirmation_window_seconds,
        idempotency_key,
        authentication,
    } = request;
    let manifest_digest = hex_decode(&manifest_digest_hex).ok_or(StatusCode::BAD_REQUEST)?;
    let confirmation_window = Duration::from_secs(confirmation_window_seconds);

    let now = SystemTime::now();
    let principal = authenticate_recovery_operation(
        &state,
        &repository_id,
        &RecoveryExecutionOperation::Preview {
            artifact_request_id: &artifact_request_id,
            manifest_digest: &manifest_digest,
            // The safety Snapshot's own receipt id and digest are not known
            // yet -- they are decided once the Snapshot actually runs,
            // below. The signature instead binds the idempotency key the
            // caller chose for it, which is what the caller can actually
            // know and commit to in advance.
            safety_snapshot_idempotency_key: &safety_snapshot_idempotency_key,
            rehearsal_id: &rehearsal_id,
            confirmation_window_seconds,
            idempotency_key: &idempotency_key,
        },
        authentication,
        now,
    )
    .await?;

    // The safety Snapshot: an ordinary platform Snapshot request/receipt,
    // triggered here rather than by a separate caller round trip. Its
    // failure fails this preview outright -- no recovery-execution request
    // row is ever created for a preview whose safety point could not be
    // captured.
    let snapshot_request = {
        use ackplane_server::administration_store::{AdministrationScope, NewSnapshotRequest};
        let administration = &state.administration;
        administration
            .request_snapshot(
                &NewSnapshotRequest {
                    policy_id: snapshot_policy_id,
                    requested_by: principal.node_id.clone(),
                    scope: AdministrationScope::Platform,
                    idempotency_key: safety_snapshot_idempotency_key,
                },
                now,
            )
            .await
            .map_err(administration_error_status)?
    };
    let existing_safety_receipt = {
        let administration = &state.administration;
        administration
            .snapshot_receipt_for_request(&snapshot_request.request.request_id)
            .await
            .map_err(administration_error_status)?
    };
    let safety_receipt = match existing_safety_receipt {
        Some(receipt) => receipt,
        None => {
            execute_and_record_snapshot(
                &state,
                &snapshot_config,
                &snapshot_request.request.request_id,
            )
            .await?
        }
    };
    let (safety_snapshot_digest, verified) = match (
        safety_receipt.manifest_digest.clone(),
        safety_receipt.verified,
    ) {
        (Some(digest), true) => (digest, true),
        _ => (Vec::new(), false),
    };
    if !verified {
        // The safety Snapshot itself failed or was not verified: decision
        // 5 makes this fatal to the preview, matching a failed `pg_dump`
        // never being silently treated as "no safety point needed."
        return Err(StatusCode::CONFLICT);
    }

    let preview_request = RecoveryExecutionPreviewRequest {
        policy_id,
        requested_by: principal.signing_key_id,
        tenant_id: state.tenant_id.to_string(),
        requesting_node_id: principal.node_id,
        requesting_public_key_fingerprint: principal.public_key_fingerprint,
        artifact_request_id,
        manifest_digest,
        safety_snapshot_receipt_id: safety_receipt.receipt_id,
        safety_snapshot_digest,
        rehearsal_id,
        confirmation_window,
        idempotency_key,
    };
    let administration = &state.administration;
    let outcome = administration
        .preview_recovery_execution(&preview_request, now)
        .await
        .map_err(administration_error_status)?;
    let idempotent_replay = outcome.idempotent_replay;
    let mut response: RecoveryExecutionPreviewResponse = outcome.request.into();
    response.idempotent_replay = idempotent_replay;
    Ok(Json(response))
}

#[derive(Deserialize)]
pub(super) struct ConfirmRecoveryExecutionRequest {
    repository_id: String,
    authentication: SignedRecoveryAuthentication,
}

#[derive(Serialize)]
pub(super) struct RecoveryConfirmationResponse {
    confirmation_id: String,
    outcome: &'static str,
    reason: String,
    occurred_at_seconds: Option<u64>,
    confirming_signing_key_id: Option<String>,
    confirming_node_id: Option<String>,
    confirming_public_key_fingerprint: Option<String>,
}

impl From<RecoveryConfirmation> for RecoveryConfirmationResponse {
    fn from(confirmation: RecoveryConfirmation) -> Self {
        Self {
            confirmation_id: confirmation.confirmation_id,
            outcome: recovery_confirmation_outcome_label(confirmation.outcome),
            reason: confirmation.reason,
            occurred_at_seconds: unix_seconds(confirmation.occurred_at),
            confirming_signing_key_id: confirmation.confirming_signing_key_id,
            confirming_node_id: confirmation.confirming_node_id,
            confirming_public_key_fingerprint: confirmation.confirming_public_key_fingerprint,
        }
    }
}

/// Authorizes (never executes) a previously previewed recovery execution.
/// The confirming key must be a second, distinct enrolled credential from
/// the one that created the preview (ADR-0145 decision 4).
pub(super) async fn confirm_recovery_execution(
    State(state): State<AdministrationApiState>,
    Path(request_id): Path<String>,
    Json(body): Json<ConfirmRecoveryExecutionRequest>,
) -> Result<Json<RecoveryConfirmationResponse>, StatusCode> {
    let now = SystemTime::now();
    // Disclosure of a request/confirmation stays bounded to the tenant that
    // made it, exactly like Lifecycle purge, even though the operation
    // itself is always platform-scoped (ADR-0145 decision 6).
    {
        let administration = &state.administration;
        let request = administration
            .recovery_execution_request(&request_id)
            .await
            .map_err(administration_error_status)?
            .ok_or(StatusCode::NOT_FOUND)?;
        if request.tenant_id != state.tenant_id.as_ref() {
            return Err(StatusCode::NOT_FOUND);
        }
    }
    let principal = authenticate_recovery_operation(
        &state,
        &body.repository_id,
        &RecoveryExecutionOperation::Confirm {
            request_id: &request_id,
        },
        body.authentication,
        now,
    )
    .await?;
    let administration = &state.administration;
    let confirmation = administration
        .confirm_recovery_execution(
            &request_id,
            &principal.signing_key_id,
            &principal.node_id,
            &principal.public_key_fingerprint,
            now,
        )
        .await
        .map_err(administration_error_status)?;
    Ok(Json(confirmation.into()))
}

pub(super) async fn recovery_execution_status(
    State(state): State<AdministrationApiState>,
    Path(request_id): Path<String>,
) -> Result<Json<RecoveryExecutionPreviewResponse>, StatusCode> {
    let administration = &state.administration;
    let request = administration
        .recovery_execution_request(&request_id)
        .await
        .map_err(administration_error_status)?
        .ok_or(StatusCode::NOT_FOUND)?;
    // Disclosure is tenant-bounded even though the request itself is always
    // platform-scoped (ADR-0145 decision 6) -- the same rule
    // `platform_snapshot_receipt` already applies to Snapshot receipts.
    if request.tenant_id != state.tenant_id.as_ref() {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(Json(request.into()))
}

async fn authenticate_recovery_operation(
    state: &AdministrationApiState,
    repository_id: &str,
    operation: &RecoveryExecutionOperation<'_>,
    authentication: SignedRecoveryAuthentication,
    now: SystemTime,
) -> Result<RecoveryPrincipal, StatusCode> {
    let authentication = authentication.into_claim_authentication();
    let claims = state
        .claims
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let binding = EnvelopeBinding {
        signing_key_id: &authentication.signing_key_id,
        tenant_id: state.tenant_id.as_ref(),
        repository_id,
        producer_id: &authentication.node_id,
        accepted_at: now,
    };
    let bytes = recovery_execution_signing_bytes(
        state.tenant_id.as_ref(),
        repository_id,
        operation,
        &authentication,
    );
    let resolution = claims
        .resolve_signing_key(&binding)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Bridge Recovery-execution signing-key lookup failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let public_key_fingerprint = match &resolution {
        KeyResolution::Resolved(record) => record.public_key_fingerprint.clone(),
        _ => String::new(),
    };
    claim_signature::verify_signed_bytes(&authentication, &resolution, &bytes, now)
        .map_err(recovery_authentication_status)?;
    let consumed = claims
        .consume_claim_nonce(&authentication.signing_key_id, &authentication.nonce, now)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Bridge Recovery-execution nonce write failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if !consumed {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(RecoveryPrincipal {
        signing_key_id: authentication.signing_key_id,
        node_id: authentication.node_id,
        public_key_fingerprint,
    })
}

fn recovery_authentication_status(refusal: ClaimAuthRefusal) -> StatusCode {
    if refusal.is_authenticated_but_not_authorized() {
        StatusCode::FORBIDDEN
    } else {
        StatusCode::UNAUTHORIZED
    }
}

fn recovery_confirmation_outcome_label(outcome: RecoveryConfirmationOutcome) -> &'static str {
    match outcome {
        RecoveryConfirmationOutcome::Confirmed => "confirmed",
        RecoveryConfirmationOutcome::Refused => "refused",
        RecoveryConfirmationOutcome::Expired => "expired",
    }
}

pub(super) fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_decode(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
        .collect()
}

pub(super) fn unix_seconds(timestamp: SystemTime) -> Option<u64> {
    timestamp
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}
