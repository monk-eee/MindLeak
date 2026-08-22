//! Durable enrollment requests and their authority-owned transitions.

use std::time::{Duration, SystemTime};

use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio_postgres::{Client, NoTls};

use thiserror::Error;

use crate::enrollment::{
    key_rotation_bytes, public_key_fingerprint, verify_activation_proof,
    verify_key_rotation_signature, ActivationProofBinding, EnrollmentState, KeyRotationStatement,
};
use crate::signing_keys::{self, KeyResolution, SigningKeyRecord};

const MIGRATION: &str = include_str!("../../migrations/0003_enrollment.sql");
const SIGNING_KEY_MIGRATION: &str = include_str!("../../migrations/0004_signing_keys.sql");
pub const ACTIVATION_CHALLENGE_LIFETIME: Duration = Duration::from_secs(300);
/// The most a retiring key may still settle in-flight records for after its
/// successor takes over (ADR-0085 decision 7's "bounded overlap").
pub const MAX_ROTATION_OVERLAP: Duration = Duration::from_secs(24 * 60 * 60);

/// Immutable information a node presents when requesting enrollment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrollmentSubmission {
    pub request_id: String,
    pub tenant_id: String,
    pub repository_id: String,
    pub proposed_node_id: String,
    pub display_name: String,
    pub public_key: Vec<u8>,
    pub public_key_fingerprint: String,
    pub requested_capabilities: Vec<String>,
    pub created_at: String,
    pub expires_at: String,
}

/// The durable state returned for an idempotent enrollment submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrollmentStatus {
    pub request_id: String,
    pub state: EnrollmentState,
}

/// An authenticated administrator's approval of a specific pending binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrollmentApproval {
    pub request_id: String,
    pub tenant_id: String,
    pub repository_id: String,
    pub public_key_fingerprint: String,
    pub approved_capabilities: Vec<String>,
    pub approved_by: String,
}

/// The exact binding a node must present when asking for a proof challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationChallengeRequest {
    pub request_id: String,
    pub tenant_id: String,
    pub repository_id: String,
    pub proposed_node_id: String,
    pub public_key_fingerprint: String,
}

/// A short-lived, single-use proof-of-possession challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedActivationChallenge {
    pub request: ActivationChallengeRequest,
    pub nonce: Vec<u8>,
    pub issued_at: SystemTime,
    pub expires_at: SystemTime,
    pub state: EnrollmentState,
}

/// The node's signed response to a previously issued challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrollmentActivation {
    pub request: ActivationChallengeRequest,
    pub nonce: Vec<u8>,
    pub signature: Vec<u8>,
}

/// The durable result of an accepted activation proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrollmentActivationResult {
    pub request_id: String,
    pub state: EnrollmentState,
    pub enrollment_receipt_id: String,
    pub signing_key_id: String,
}

/// A node's request to replace its current signing key with a successor it
/// already holds, proving continuity of both (ADR-0085 decision 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRotation {
    pub tenant_id: String,
    pub repository_id: String,
    pub node_id: String,
    pub current_key_id: String,
    pub successor_key_id: String,
    pub successor_public_key_fingerprint: String,
    pub successor_public_key: Vec<u8>,
    pub current_key_signature: Vec<u8>,
    pub successor_key_signature: Vec<u8>,
    pub requested_overlap_seconds: u64,
}

/// Why a rotation could not be applied. Each variant matches one wire
/// `KeyRotationRejectionReason` (ADR-0085 decision 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRotationRejection {
    CurrentKeyNotActive,
    SuccessorKeyConflict,
    ContinuityProofInvalid,
    NodeRevoked,
}

/// Whether a rotation was applied, folding the rejection reason into the one
/// case it applies to rather than carrying a separate, sometimes-meaningless
/// reason field alongside an "applied" outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRotationOutcome {
    Applied,
    Rejected(KeyRotationRejection),
}

/// The durable result of a rotation attempt, applied or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRotationResult {
    pub node_id: String,
    pub current_key_id: String,
    pub successor_key_id: String,
    pub outcome: KeyRotationOutcome,
}

#[derive(Debug, Error)]
pub enum EnrollmentStoreError {
    #[error("enrollment request {request_id} conflicts with the binding already recorded")]
    RequestConflict { request_id: String },
    #[error("enrollment request {request_id} does not name the fingerprint of its public key")]
    PublicKeyFingerprintMismatch { request_id: String },
    #[error("enrollment request {request_id} was not found")]
    NotFound { request_id: String },
    #[error("enrollment request {request_id} is not pending")]
    NotPending { request_id: String },
    #[error("the approved fingerprint does not match enrollment request {request_id}")]
    FingerprintMismatch { request_id: String },
    #[error("enrollment request {request_id} does not match the supplied binding")]
    BindingMismatch { request_id: String },
    #[error("enrollment request {request_id} is not approved")]
    NotApproved { request_id: String },
    #[error("enrollment request {request_id} has expired")]
    RequestExpired { request_id: String },
    #[error("activation challenge for enrollment request {request_id} has expired")]
    ChallengeExpired { request_id: String },
    #[error("activation challenge for enrollment request {request_id} was already consumed")]
    ChallengeConsumed { request_id: String },
    #[error("activation proof for enrollment request {request_id} is invalid")]
    InvalidProof { request_id: String },
    #[error("enrollment database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("registering the activated signing key failed: {0}")]
    SigningKey(#[from] crate::signing_keys::SigningKeyError),
    #[error("enrollment request contains an unknown stored state {value}")]
    UnknownState { value: i16 },
    #[error("enrollment request {request_id} has a malformed {field} timestamp {value:?}")]
    InvalidTimestamp {
        request_id: String,
        field: &'static str,
        value: String,
    },
}

/// A connection to the authoritative enrollment state and audit history.
pub struct EnrollmentStore {
    client: Client,
}

impl EnrollmentStore {
    /// Connect and apply the idempotent enrollment schema.
    pub async fn connect(database_url: &str) -> Result<Self, tokio_postgres::Error> {
        let (mut client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!(%error, "ackplane enrollment connection closed with an error");
            }
        });
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::ENROLLMENT,
            MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::SIGNING_KEYS,
            SIGNING_KEY_MIGRATION,
        )
        .await?;
        Ok(Self { client })
    }
}

mod activation;
mod rotation;
mod submission;

fn rejected(rotation: &KeyRotation, reason: KeyRotationRejection) -> KeyRotationResult {
    KeyRotationResult {
        node_id: rotation.node_id.clone(),
        current_key_id: rotation.current_key_id.clone(),
        successor_key_id: rotation.successor_key_id.clone(),
        outcome: KeyRotationOutcome::Rejected(reason),
    }
}

fn validate_binding(
    request: &ActivationChallengeRequest,
    proposed_node_id: String,
    public_key_fingerprint: String,
) -> Result<(), EnrollmentStoreError> {
    if request.proposed_node_id == proposed_node_id
        && request.public_key_fingerprint == public_key_fingerprint
    {
        Ok(())
    } else {
        Err(EnrollmentStoreError::BindingMismatch {
            request_id: request.request_id.clone(),
        })
    }
}

/// Parse an RFC3339 timestamp into the typed value the `timestamptz` column
/// requires. A `String` bound directly against that column fails Postgres's
/// own type check (it describes the parameter as `timestamptz`, and `String`
/// implements `ToSql` only for text-like types), so this must happen in Rust
/// before the value ever reaches a query.
fn parse_rfc3339(
    request_id: &str,
    field: &'static str,
    value: &str,
) -> Result<SystemTime, EnrollmentStoreError> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map(SystemTime::from)
        .map_err(|_| EnrollmentStoreError::InvalidTimestamp {
            request_id: request_id.to_owned(),
            field,
            value: value.to_owned(),
        })
}

async fn expire_enrollment(
    transaction: &tokio_postgres::Transaction<'_>,
    request_id: &str,
    tenant_id: &str,
    repository_id: &str,
    proposed_node_id: &str,
    public_key_fingerprint: &str,
) -> Result<(), tokio_postgres::Error> {
    transaction
        .execute(
            "UPDATE enrollment_requests SET state = $4 WHERE tenant_id = $1 \
             AND repository_id = $2 AND request_id = $3",
            &[
                &tenant_id,
                &repository_id,
                &request_id,
                &state_as_i16(EnrollmentState::Expired),
            ],
        )
        .await?;
    transaction
        .execute(
            "INSERT INTO enrollment_transitions (
                 tenant_id, repository_id, request_id, proposed_node_id,
                 public_key_fingerprint, state, actor, reason
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
            &[
                &tenant_id,
                &repository_id,
                &request_id,
                &proposed_node_id,
                &public_key_fingerprint,
                &state_as_i16(EnrollmentState::Expired),
                &"ackplane-enrollment-authority",
                &"enrollment request expired",
            ],
        )
        .await?;
    Ok(())
}

fn state_as_i16(state: EnrollmentState) -> i16 {
    match state {
        EnrollmentState::Pending => 1,
        EnrollmentState::Approved => 2,
        EnrollmentState::Activating => 3,
        EnrollmentState::Active => 4,
        EnrollmentState::Expired => 5,
        EnrollmentState::Rejected => 6,
        EnrollmentState::Revoked => 7,
    }
}

fn state_from_i16(value: i16) -> Result<EnrollmentState, EnrollmentStoreError> {
    match value {
        1 => Ok(EnrollmentState::Pending),
        2 => Ok(EnrollmentState::Approved),
        3 => Ok(EnrollmentState::Activating),
        4 => Ok(EnrollmentState::Active),
        5 => Ok(EnrollmentState::Expired),
        6 => Ok(EnrollmentState::Rejected),
        7 => Ok(EnrollmentState::Revoked),
        _ => Err(EnrollmentStoreError::UnknownState { value }),
    }
}

async fn append_transition(
    transaction: &tokio_postgres::Transaction<'_>,
    submission: &EnrollmentSubmission,
    state: EnrollmentState,
    actor: &str,
    reason: &str,
) -> Result<(), tokio_postgres::Error> {
    transaction
        .execute(
            "INSERT INTO enrollment_transitions (
                 tenant_id, repository_id, request_id, proposed_node_id,
                 public_key_fingerprint, state, actor, reason
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
            &[
                &submission.tenant_id,
                &submission.repository_id,
                &submission.request_id,
                &submission.proposed_node_id,
                &submission.public_key_fingerprint,
                &state_as_i16(state),
                &actor,
                &reason,
            ],
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: `submit` used to bind `created_at`/`expires_at` straight
    /// into a `timestamptz` column as raw text with a SQL-side `::timestamptz`
    /// cast. Postgres describes that parameter's type from the prepared
    /// statement, so the driver rejected every bound `String` with a
    /// `WrongType` error before the query ever ran — every enrollment
    /// submission against a real database failed, DB-gated tests included.
    /// This needs no database: the defect was in Rust-side type conversion,
    /// not in anything only Postgres could tell us.
    #[test]
    fn a_wellformed_rfc3339_timestamp_parses_to_the_same_instant() {
        let parsed = parse_rfc3339("request-1", "created_at", "2026-01-01T00:00:00Z")
            .expect("a valid RFC3339 timestamp parses");

        assert_eq!(
            parsed,
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_225_600)
        );
    }

    #[test]
    fn a_malformed_timestamp_is_rejected_before_it_ever_reaches_a_query() {
        let error = parse_rfc3339("request-1", "expires_at", "not-a-timestamp")
            .expect_err("a malformed timestamp must not parse");

        match error {
            EnrollmentStoreError::InvalidTimestamp {
                request_id,
                field,
                value,
            } => {
                assert_eq!(request_id, "request-1");
                assert_eq!(field, "expires_at");
                assert_eq!(value, "not-a-timestamp");
            }
            other => panic!("expected InvalidTimestamp, got {other:?}"),
        }
    }
}
