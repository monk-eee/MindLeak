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

const MIGRATION: &str = include_str!("../migrations/0003_enrollment.sql");
const SIGNING_KEY_MIGRATION: &str = include_str!("../migrations/0004_signing_keys.sql");
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
        let (client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!(%error, "ackplane enrollment connection closed with an error");
            }
        });
        client.batch_execute(MIGRATION).await?;
        client.batch_execute(SIGNING_KEY_MIGRATION).await?;
        Ok(Self { client })
    }

    /// Persist a pending request or return its already-recorded state when a
    /// node retries the exact same request id and binding.
    pub async fn submit(
        &mut self,
        submission: &EnrollmentSubmission,
    ) -> Result<EnrollmentStatus, EnrollmentStoreError> {
        if public_key_fingerprint(&submission.public_key) != submission.public_key_fingerprint {
            return Err(EnrollmentStoreError::PublicKeyFingerprintMismatch {
                request_id: submission.request_id.clone(),
            });
        }
        let transaction = self.client.transaction().await?;
        let existing = transaction
            .query_opt(
                "SELECT proposed_node_id, display_name, public_key, public_key_fingerprint, \
                 requested_capabilities, state FROM enrollment_requests \
                 WHERE tenant_id = $1 AND repository_id = $2 AND request_id = $3 FOR UPDATE",
                &[
                    &submission.tenant_id,
                    &submission.repository_id,
                    &submission.request_id,
                ],
            )
            .await?;

        if let Some(row) = existing {
            let existing_state = state_from_i16(row.get(5))?;
            let matches = row.get::<_, String>(0) == submission.proposed_node_id
                && row.get::<_, String>(1) == submission.display_name
                && row.get::<_, Vec<u8>>(2) == submission.public_key
                && row.get::<_, String>(3) == submission.public_key_fingerprint
                && row.get::<_, Vec<String>>(4) == submission.requested_capabilities;
            if !matches {
                return Err(EnrollmentStoreError::RequestConflict {
                    request_id: submission.request_id.clone(),
                });
            }
            return Ok(EnrollmentStatus {
                request_id: submission.request_id.clone(),
                state: existing_state,
            });
        }

        let created_at =
            parse_rfc3339(&submission.request_id, "created_at", &submission.created_at)?;
        let expires_at =
            parse_rfc3339(&submission.request_id, "expires_at", &submission.expires_at)?;
        transaction
            .execute(
                "INSERT INTO enrollment_requests (
                     tenant_id, repository_id, request_id, proposed_node_id, display_name,
                     public_key, public_key_fingerprint, requested_capabilities, created_at,
                     expires_at, state
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
                &[
                    &submission.tenant_id,
                    &submission.repository_id,
                    &submission.request_id,
                    &submission.proposed_node_id,
                    &submission.display_name,
                    &submission.public_key,
                    &submission.public_key_fingerprint,
                    &submission.requested_capabilities,
                    &created_at,
                    &expires_at,
                    &state_as_i16(EnrollmentState::Pending),
                ],
            )
            .await?;
        append_transition(
            &transaction,
            submission,
            EnrollmentState::Pending,
            "unauthenticated-node-request",
            "enrollment requested",
        )
        .await?;
        transaction.commit().await?;

        Ok(EnrollmentStatus {
            request_id: submission.request_id.clone(),
            state: EnrollmentState::Pending,
        })
    }

    /// Record an administrator's approval of the exact fingerprint it reviewed.
    pub async fn approve(
        &mut self,
        approval: &EnrollmentApproval,
    ) -> Result<EnrollmentStatus, EnrollmentStoreError> {
        let transaction = self.client.transaction().await?;
        let row = transaction
            .query_opt(
                "SELECT proposed_node_id, public_key_fingerprint, state, expires_at FROM enrollment_requests \
                 WHERE tenant_id = $1 AND repository_id = $2 AND request_id = $3 FOR UPDATE",
                &[
                    &approval.tenant_id,
                    &approval.repository_id,
                    &approval.request_id,
                ],
            )
            .await?
            .ok_or_else(|| EnrollmentStoreError::NotFound {
                request_id: approval.request_id.clone(),
            })?;
        let node_id: String = row.get(0);
        let stored_fingerprint: String = row.get(1);
        let stored_state = state_from_i16(row.get(2))?;
        let expires_at: SystemTime = row.get(3);
        if SystemTime::now() > expires_at {
            expire_enrollment(
                &transaction,
                &approval.request_id,
                &approval.tenant_id,
                &approval.repository_id,
                &node_id,
                &stored_fingerprint,
            )
            .await?;
            transaction.commit().await?;
            return Err(EnrollmentStoreError::RequestExpired {
                request_id: approval.request_id.clone(),
            });
        }
        if stored_state != EnrollmentState::Pending {
            return Err(EnrollmentStoreError::NotPending {
                request_id: approval.request_id.clone(),
            });
        }
        if stored_fingerprint != approval.public_key_fingerprint {
            return Err(EnrollmentStoreError::FingerprintMismatch {
                request_id: approval.request_id.clone(),
            });
        }

        transaction
            .execute(
                "UPDATE enrollment_requests SET state = $4, approved_fingerprint = $5, \
                 approved_capabilities = $6, approved_at = now(), approved_by = $7 \
                 WHERE tenant_id = $1 AND repository_id = $2 AND request_id = $3",
                &[
                    &approval.tenant_id,
                    &approval.repository_id,
                    &approval.request_id,
                    &state_as_i16(EnrollmentState::Approved),
                    &approval.public_key_fingerprint,
                    &approval.approved_capabilities,
                    &approval.approved_by,
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
                    &approval.tenant_id,
                    &approval.repository_id,
                    &approval.request_id,
                    &node_id,
                    &approval.public_key_fingerprint,
                    &state_as_i16(EnrollmentState::Approved),
                    &approval.approved_by,
                    &"fingerprint approved",
                ],
            )
            .await?;
        transaction.commit().await?;

        Ok(EnrollmentStatus {
            request_id: approval.request_id.clone(),
            state: EnrollmentState::Approved,
        })
    }

    /// Return the currently valid challenge for an approved enrollment, or
    /// record a fresh supplied nonce after the prior challenge expires. The
    /// caller generates the nonce with its operating-system CSPRNG.
    pub async fn issue_challenge(
        &mut self,
        request: &ActivationChallengeRequest,
        nonce: &[u8],
        now: SystemTime,
    ) -> Result<IssuedActivationChallenge, EnrollmentStoreError> {
        let transaction = self.client.transaction().await?;
        let enrollment = transaction
            .query_opt(
                "SELECT proposed_node_id, public_key_fingerprint, state, expires_at FROM enrollment_requests \
                 WHERE tenant_id = $1 AND repository_id = $2 AND request_id = $3 FOR UPDATE",
                &[&request.tenant_id, &request.repository_id, &request.request_id],
            )
            .await?
            .ok_or_else(|| EnrollmentStoreError::NotFound {
                request_id: request.request_id.clone(),
            })?;
        validate_binding(
            request,
            enrollment.get::<_, String>(0),
            enrollment.get::<_, String>(1),
        )?;
        let request_expires_at: SystemTime = enrollment.get(3);
        if now > request_expires_at {
            expire_enrollment(
                &transaction,
                &request.request_id,
                &request.tenant_id,
                &request.repository_id,
                &request.proposed_node_id,
                &request.public_key_fingerprint,
            )
            .await?;
            transaction.commit().await?;
            return Err(EnrollmentStoreError::RequestExpired {
                request_id: request.request_id.clone(),
            });
        }
        if state_from_i16(enrollment.get(2))? != EnrollmentState::Approved {
            return Err(EnrollmentStoreError::NotApproved {
                request_id: request.request_id.clone(),
            });
        }

        let existing = transaction
            .query_opt(
                "SELECT nonce, issued_at, expires_at FROM activation_challenges \
                 WHERE tenant_id = $1 AND repository_id = $2 AND request_id = $3 FOR UPDATE",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.request_id,
                ],
            )
            .await?;
        if let Some(challenge) = existing {
            let expires_at: SystemTime = challenge.get(2);
            if expires_at > now {
                transaction.commit().await?;
                return Ok(IssuedActivationChallenge {
                    request: request.clone(),
                    nonce: challenge.get(0),
                    issued_at: challenge.get(1),
                    expires_at,
                    state: EnrollmentState::Approved,
                });
            }
        }

        let expires_at = now + ACTIVATION_CHALLENGE_LIFETIME;
        transaction
            .execute(
                "INSERT INTO activation_challenges (
                     tenant_id, repository_id, request_id, proposed_node_id,
                     public_key_fingerprint, nonce, issued_at, expires_at
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
                 ON CONFLICT (tenant_id, repository_id, request_id) DO UPDATE SET
                     proposed_node_id = EXCLUDED.proposed_node_id,
                     public_key_fingerprint = EXCLUDED.public_key_fingerprint,
                     nonce = EXCLUDED.nonce,
                     issued_at = EXCLUDED.issued_at,
                     expires_at = EXCLUDED.expires_at,
                     consumed_at = NULL",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.request_id,
                    &request.proposed_node_id,
                    &request.public_key_fingerprint,
                    &nonce,
                    &now,
                    &expires_at,
                ],
            )
            .await?;
        transaction.commit().await?;

        Ok(IssuedActivationChallenge {
            request: request.clone(),
            nonce: nonce.to_vec(),
            issued_at: now,
            expires_at,
            state: EnrollmentState::Approved,
        })
    }

    /// Verify a proof against the stored approved key, then atomically consume
    /// its challenge, record the activating transition, and mint one receipt.
    /// An exact replay returns that receipt rather than creating fresh authority.
    pub async fn activate(
        &mut self,
        activation: &EnrollmentActivation,
        enrollment_receipt_id: &str,
        signing_key_id: &str,
        now: SystemTime,
    ) -> Result<EnrollmentActivationResult, EnrollmentStoreError> {
        let request = &activation.request;
        let transaction = self.client.transaction().await?;
        let enrollment = transaction
            .query_opt(
                "SELECT proposed_node_id, public_key_fingerprint, public_key, state, expires_at \
                 FROM enrollment_requests WHERE tenant_id = $1 AND repository_id = $2 \
                 AND request_id = $3 FOR UPDATE",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.request_id,
                ],
            )
            .await?
            .ok_or_else(|| EnrollmentStoreError::NotFound {
                request_id: request.request_id.clone(),
            })?;
        validate_binding(
            request,
            enrollment.get::<_, String>(0),
            enrollment.get::<_, String>(1),
        )?;
        let public_key: Vec<u8> = enrollment.get(2);
        let state = state_from_i16(enrollment.get(3))?;
        let request_expires_at: SystemTime = enrollment.get(4);
        if now > request_expires_at {
            expire_enrollment(
                &transaction,
                &request.request_id,
                &request.tenant_id,
                &request.repository_id,
                &request.proposed_node_id,
                &request.public_key_fingerprint,
            )
            .await?;
            transaction.commit().await?;
            return Err(EnrollmentStoreError::RequestExpired {
                request_id: request.request_id.clone(),
            });
        }

        let challenge = transaction
            .query_opt(
                "SELECT nonce, expires_at, consumed_at FROM activation_challenges \
                 WHERE tenant_id = $1 AND repository_id = $2 AND request_id = $3 FOR UPDATE",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.request_id,
                ],
            )
            .await?
            .ok_or_else(|| EnrollmentStoreError::ChallengeExpired {
                request_id: request.request_id.clone(),
            })?;
        let stored_nonce: Vec<u8> = challenge.get(0);
        let expires_at: SystemTime = challenge.get(1);
        let consumed_at: Option<SystemTime> = challenge.get(2);
        let proof_is_valid = stored_nonce == activation.nonce
            && verify_activation_proof(
                &public_key,
                &activation.signature,
                ActivationProofBinding {
                    nonce: &activation.nonce,
                    request_id: &request.request_id,
                    tenant_id: &request.tenant_id,
                    repository_id: &request.repository_id,
                    node_id: &request.proposed_node_id,
                    public_key_fingerprint: &request.public_key_fingerprint,
                },
            );
        if !proof_is_valid {
            return Err(EnrollmentStoreError::InvalidProof {
                request_id: request.request_id.clone(),
            });
        }

        if state == EnrollmentState::Activating && consumed_at.is_some() {
            let receipt = transaction
                .query_one(
                    "SELECT enrollment_receipt_id FROM enrollment_receipts WHERE tenant_id = $1 \
                     AND repository_id = $2 AND request_id = $3",
                    &[
                        &request.tenant_id,
                        &request.repository_id,
                        &request.request_id,
                    ],
                )
                .await?;
            transaction.commit().await?;
            return Ok(EnrollmentActivationResult {
                request_id: request.request_id.clone(),
                state,
                enrollment_receipt_id: receipt.get(0),
            });
        }
        if state != EnrollmentState::Approved {
            return Err(EnrollmentStoreError::NotApproved {
                request_id: request.request_id.clone(),
            });
        }
        if consumed_at.is_some() {
            return Err(EnrollmentStoreError::ChallengeConsumed {
                request_id: request.request_id.clone(),
            });
        }
        if now > expires_at {
            return Err(EnrollmentStoreError::ChallengeExpired {
                request_id: request.request_id.clone(),
            });
        }

        transaction
            .execute(
                "UPDATE activation_challenges SET consumed_at = $4 WHERE tenant_id = $1 \
                 AND repository_id = $2 AND request_id = $3",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.request_id,
                    &now,
                ],
            )
            .await?;
        transaction
            .execute(
                "UPDATE enrollment_requests SET state = $4 WHERE tenant_id = $1 \
                 AND repository_id = $2 AND request_id = $3",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.request_id,
                    &state_as_i16(EnrollmentState::Activating),
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
                    &request.tenant_id,
                    &request.repository_id,
                    &request.request_id,
                    &request.proposed_node_id,
                    &request.public_key_fingerprint,
                    &state_as_i16(EnrollmentState::Activating),
                    &"node-proof-of-possession",
                    &"activation proof verified",
                ],
            )
            .await?;
        transaction
            .execute(
                "INSERT INTO enrollment_receipts (
                     enrollment_receipt_id, tenant_id, repository_id, request_id,
                     proposed_node_id, public_key_fingerprint, activated_at
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7)",
                &[
                    &enrollment_receipt_id,
                    &request.tenant_id,
                    &request.repository_id,
                    &request.request_id,
                    &request.proposed_node_id,
                    &request.public_key_fingerprint,
                    &now,
                ],
            )
            .await?;
        // Same transaction as the receipt: a node that is activated but whose
        // key nothing can resolve would sign records no one could verify.
        signing_keys::register(
            &transaction,
            &SigningKeyRecord {
                signing_key_id: signing_key_id.to_owned(),
                tenant_id: request.tenant_id.clone(),
                repository_id: request.repository_id.clone(),
                node_id: request.proposed_node_id.clone(),
                public_key,
                public_key_fingerprint: request.public_key_fingerprint.clone(),
                activated_at: now,
                expires_at: None,
            },
        )
        .await?;
        transaction.commit().await?;

        Ok(EnrollmentActivationResult {
            request_id: request.request_id.clone(),
            state: EnrollmentState::Activating,
            enrollment_receipt_id: enrollment_receipt_id.to_owned(),
        })
    }

    /// Apply or reject a rotation, verifying continuity between the current
    /// and successor keys before either is touched (ADR-0085 decision 7).
    /// A rejection is a normal, non-error result: the wire contract carries a
    /// typed outcome precisely so a node can distinguish "try again" from
    /// "stop and re-enrol" without parsing a status message.
    pub async fn rotate_key(
        &mut self,
        rotation: &KeyRotation,
        now: SystemTime,
    ) -> Result<KeyRotationResult, EnrollmentStoreError> {
        let transaction = self.client.transaction().await?;

        let current =
            signing_keys::fetch_lifecycle_for_update(&transaction, &rotation.current_key_id)
                .await?;
        let Some(current) = current else {
            return Ok(rejected(
                rotation,
                KeyRotationRejection::CurrentKeyNotActive,
            ));
        };
        let resolution = signing_keys::judge(
            &current,
            &signing_keys::EnvelopeBinding {
                signing_key_id: &rotation.current_key_id,
                tenant_id: &rotation.tenant_id,
                repository_id: &rotation.repository_id,
                producer_id: &rotation.node_id,
                accepted_at: now,
            },
        );
        let current_public_key = match resolution {
            KeyResolution::Resolved(record) => record.public_key,
            KeyResolution::Revoked => {
                return Ok(rejected(rotation, KeyRotationRejection::NodeRevoked));
            }
            KeyResolution::Unknown
            | KeyResolution::BindingMismatch
            | KeyResolution::NotYetActive
            | KeyResolution::Expired
            | KeyResolution::Retired => {
                return Ok(rejected(
                    rotation,
                    KeyRotationRejection::CurrentKeyNotActive,
                ));
            }
        };

        if public_key_fingerprint(&rotation.successor_public_key)
            != rotation.successor_public_key_fingerprint
        {
            return Ok(rejected(
                rotation,
                KeyRotationRejection::ContinuityProofInvalid,
            ));
        }
        if signing_keys::key_exists(&transaction, &rotation.successor_key_id).await? {
            return Ok(rejected(
                rotation,
                KeyRotationRejection::SuccessorKeyConflict,
            ));
        }

        let statement = key_rotation_bytes(&KeyRotationStatement {
            tenant_id: &rotation.tenant_id,
            repository_id: &rotation.repository_id,
            node_id: &rotation.node_id,
            current_key_id: &rotation.current_key_id,
            successor_key_id: &rotation.successor_key_id,
            successor_public_key_fingerprint: &rotation.successor_public_key_fingerprint,
            successor_public_key: &rotation.successor_public_key,
            requested_overlap_seconds: rotation.requested_overlap_seconds,
        });
        let current_signed = verify_key_rotation_signature(
            &current_public_key,
            &rotation.current_key_signature,
            &statement,
        );
        let successor_signed = verify_key_rotation_signature(
            &rotation.successor_public_key,
            &rotation.successor_key_signature,
            &statement,
        );
        if !current_signed || !successor_signed {
            return Ok(rejected(
                rotation,
                KeyRotationRejection::ContinuityProofInvalid,
            ));
        }

        let overlap =
            Duration::from_secs(rotation.requested_overlap_seconds).min(MAX_ROTATION_OVERLAP);
        let retired_at = now + overlap;
        signing_keys::retire(&transaction, &rotation.current_key_id, retired_at).await?;
        signing_keys::register(
            &transaction,
            &SigningKeyRecord {
                signing_key_id: rotation.successor_key_id.clone(),
                tenant_id: rotation.tenant_id.clone(),
                repository_id: rotation.repository_id.clone(),
                node_id: rotation.node_id.clone(),
                public_key: rotation.successor_public_key.clone(),
                public_key_fingerprint: rotation.successor_public_key_fingerprint.clone(),
                activated_at: now,
                expires_at: None,
            },
        )
        .await?;
        transaction.commit().await?;

        Ok(KeyRotationResult {
            node_id: rotation.node_id.clone(),
            current_key_id: rotation.current_key_id.clone(),
            successor_key_id: rotation.successor_key_id.clone(),
            outcome: KeyRotationOutcome::Applied,
        })
    }
}

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
    use std::time::{SystemTime, UNIX_EPOCH};

    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::enrollment::activation_challenge_bytes;

    fn submission() -> EnrollmentSubmission {
        submission_for(&SigningKey::from_bytes(&[7; 32]))
    }

    fn submission_for(signing_key: &SigningKey) -> EnrollmentSubmission {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos();
        EnrollmentSubmission {
            request_id: format!("request-{unique_suffix}"),
            tenant_id: "tenant-test".to_owned(),
            repository_id: "repository-test".to_owned(),
            proposed_node_id: format!("node-{unique_suffix}"),
            display_name: "Node test".to_owned(),
            public_key: signing_key.verifying_key().to_bytes().to_vec(),
            public_key_fingerprint: public_key_fingerprint(&signing_key.verifying_key().to_bytes()),
            requested_capabilities: vec!["synchronize".to_owned()],
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            expires_at: "2030-01-01T00:00:00Z".to_owned(),
        }
    }

    #[tokio::test]
    async fn exact_retry_observes_the_approved_enrollment_state() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let mut store = EnrollmentStore::connect(&database_url)
            .await
            .expect("test database connects");
        let enrollment = submission();

        let first = store.submit(&enrollment).await.expect("request persists");
        let retry = store
            .submit(&enrollment)
            .await
            .expect("retry is idempotent");
        let approved = store
            .approve(&EnrollmentApproval {
                request_id: enrollment.request_id.clone(),
                tenant_id: enrollment.tenant_id.clone(),
                repository_id: enrollment.repository_id.clone(),
                public_key_fingerprint: enrollment.public_key_fingerprint.clone(),
                approved_capabilities: enrollment.requested_capabilities.clone(),
                approved_by: "administrator-test".to_owned(),
            })
            .await
            .expect("exact fingerprint approval succeeds");
        let retry_after_approval = store
            .submit(&enrollment)
            .await
            .expect("retry observes current state");

        assert_eq!(
            (first, retry, approved, retry_after_approval),
            (
                EnrollmentStatus {
                    request_id: enrollment.request_id.clone(),
                    state: EnrollmentState::Pending,
                },
                EnrollmentStatus {
                    request_id: enrollment.request_id.clone(),
                    state: EnrollmentState::Pending,
                },
                EnrollmentStatus {
                    request_id: enrollment.request_id.clone(),
                    state: EnrollmentState::Approved,
                },
                EnrollmentStatus {
                    request_id: enrollment.request_id,
                    state: EnrollmentState::Approved,
                },
            )
        );
    }

    #[tokio::test]
    async fn activation_reuses_its_live_challenge_and_exact_replay_receipt() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let signing_key = SigningKey::from_bytes(&[8; 32]);
        let enrollment = submission_for(&signing_key);
        let request = ActivationChallengeRequest {
            request_id: enrollment.request_id.clone(),
            tenant_id: enrollment.tenant_id.clone(),
            repository_id: enrollment.repository_id.clone(),
            proposed_node_id: enrollment.proposed_node_id.clone(),
            public_key_fingerprint: enrollment.public_key_fingerprint.clone(),
        };
        let approval = EnrollmentApproval {
            request_id: enrollment.request_id.clone(),
            tenant_id: enrollment.tenant_id.clone(),
            repository_id: enrollment.repository_id.clone(),
            public_key_fingerprint: enrollment.public_key_fingerprint.clone(),
            approved_capabilities: enrollment.requested_capabilities.clone(),
            approved_by: "administrator-test".to_owned(),
        };
        let now = SystemTime::now();
        let mut store = EnrollmentStore::connect(&database_url)
            .await
            .expect("test database connects");
        store.submit(&enrollment).await.expect("request persists");
        store.approve(&approval).await.expect("request is approved");

        let challenge = store
            .issue_challenge(&request, &[1; 32], now)
            .await
            .expect("approved request receives challenge");
        let challenge_retry = store
            .issue_challenge(&request, &[2; 32], now)
            .await
            .expect("live challenge is returned on retry");
        let signature = signing_key.sign(&activation_challenge_bytes(
            &challenge.nonce,
            &request.request_id,
            &request.tenant_id,
            &request.repository_id,
            &request.proposed_node_id,
            &request.public_key_fingerprint,
        ));
        let activation = EnrollmentActivation {
            request,
            nonce: challenge.nonce.clone(),
            signature: signature.to_bytes().to_vec(),
        };
        let first = store
            .activate(&activation, "receipt-original", "signing-key-original", now)
            .await
            .expect("valid proof activates enrollment");
        let replay = store
            .activate(
                &activation,
                "receipt-replay-must-not-persist",
                "signing-key-replay-must-not-persist",
                now,
            )
            .await
            .expect("exact valid replay returns durable receipt");

        assert_eq!(
            (challenge.nonce, challenge_retry.nonce, first, replay),
            (
                vec![1; 32],
                vec![1; 32],
                EnrollmentActivationResult {
                    request_id: enrollment.request_id.clone(),
                    state: EnrollmentState::Activating,
                    enrollment_receipt_id: "receipt-original".to_owned(),
                },
                EnrollmentActivationResult {
                    request_id: enrollment.request_id,
                    state: EnrollmentState::Activating,
                    enrollment_receipt_id: "receipt-original".to_owned(),
                },
            )
        );
    }

    /// Submit, approve, challenge and activate one node end to end, leaving it
    /// with a live signing key a rotation test can then act on.
    async fn activated_node(
        store: &mut EnrollmentStore,
        signing_key: &SigningKey,
        signing_key_id: &str,
        now: SystemTime,
    ) -> EnrollmentSubmission {
        let enrollment = submission_for(signing_key);
        store.submit(&enrollment).await.expect("request persists");
        store
            .approve(&EnrollmentApproval {
                request_id: enrollment.request_id.clone(),
                tenant_id: enrollment.tenant_id.clone(),
                repository_id: enrollment.repository_id.clone(),
                public_key_fingerprint: enrollment.public_key_fingerprint.clone(),
                approved_capabilities: enrollment.requested_capabilities.clone(),
                approved_by: "administrator-test".to_owned(),
            })
            .await
            .expect("request is approved");
        let request = ActivationChallengeRequest {
            request_id: enrollment.request_id.clone(),
            tenant_id: enrollment.tenant_id.clone(),
            repository_id: enrollment.repository_id.clone(),
            proposed_node_id: enrollment.proposed_node_id.clone(),
            public_key_fingerprint: enrollment.public_key_fingerprint.clone(),
        };
        let challenge = store
            .issue_challenge(&request, &nonce_for(signing_key_id), now)
            .await
            .expect("approved request receives challenge");
        let signature = signing_key.sign(&activation_challenge_bytes(
            &challenge.nonce,
            &request.request_id,
            &request.tenant_id,
            &request.repository_id,
            &request.proposed_node_id,
            &request.public_key_fingerprint,
        ));
        store
            .activate(
                &EnrollmentActivation {
                    request,
                    nonce: challenge.nonce,
                    signature: signature.to_bytes().to_vec(),
                },
                &format!("receipt-{signing_key_id}"),
                signing_key_id,
                now,
            )
            .await
            .expect("valid proof activates enrollment");
        enrollment
    }

    fn signed_rotation(
        current: &SigningKey,
        successor: &SigningKey,
        rotation: KeyRotation,
    ) -> KeyRotation {
        let statement = key_rotation_bytes(&KeyRotationStatement {
            tenant_id: &rotation.tenant_id,
            repository_id: &rotation.repository_id,
            node_id: &rotation.node_id,
            current_key_id: &rotation.current_key_id,
            successor_key_id: &rotation.successor_key_id,
            successor_public_key_fingerprint: &rotation.successor_public_key_fingerprint,
            successor_public_key: &rotation.successor_public_key,
            requested_overlap_seconds: rotation.requested_overlap_seconds,
        });
        KeyRotation {
            current_key_signature: current.sign(&statement).to_bytes().to_vec(),
            successor_key_signature: successor.sign(&statement).to_bytes().to_vec(),
            ..rotation
        }
    }

    #[tokio::test]
    async fn a_continuity_proven_rotation_retires_the_old_key_and_activates_the_new_one() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let now = SystemTime::now();
        let mut store = EnrollmentStore::connect(&database_url)
            .await
            .expect("test database connects");
        let current_key = SigningKey::from_bytes(&[11; 32]);
        let successor_key = SigningKey::from_bytes(&[12; 32]);
        let signing_key_id = format!("signing-key-rotation-current-{}", node_suffix());
        let successor_key_id = format!("signing-key-rotation-successor-{}", node_suffix());
        let enrollment = activated_node(&mut store, &current_key, &signing_key_id, now).await;

        let rotation = signed_rotation(
            &current_key,
            &successor_key,
            KeyRotation {
                tenant_id: enrollment.tenant_id.clone(),
                repository_id: enrollment.repository_id.clone(),
                node_id: enrollment.proposed_node_id.clone(),
                current_key_id: signing_key_id.clone(),
                successor_key_id: successor_key_id.clone(),
                successor_public_key_fingerprint: public_key_fingerprint(
                    &successor_key.verifying_key().to_bytes(),
                ),
                successor_public_key: successor_key.verifying_key().to_bytes().to_vec(),
                current_key_signature: Vec::new(),
                successor_key_signature: Vec::new(),
                requested_overlap_seconds: 3_600,
            },
        );

        let result = store
            .rotate_key(&rotation, now)
            .await
            .expect("rotation with a valid continuity proof applies");

        assert_eq!(
            result,
            KeyRotationResult {
                node_id: enrollment.proposed_node_id,
                current_key_id: signing_key_id.clone(),
                successor_key_id: successor_key_id.clone(),
                outcome: KeyRotationOutcome::Applied,
            }
        );

        let current_after = signing_keys::resolve(
            &store.client,
            &signing_keys::EnvelopeBinding {
                signing_key_id: &signing_key_id,
                tenant_id: &rotation.tenant_id,
                repository_id: &rotation.repository_id,
                producer_id: &rotation.node_id,
                accepted_at: now + Duration::from_secs(7_200),
            },
        )
        .await
        .expect("resolve queries succeed");
        let successor_after = signing_keys::resolve(
            &store.client,
            &signing_keys::EnvelopeBinding {
                signing_key_id: &successor_key_id,
                tenant_id: &rotation.tenant_id,
                repository_id: &rotation.repository_id,
                producer_id: &rotation.node_id,
                accepted_at: now,
            },
        )
        .await
        .expect("resolve queries succeed");

        assert_eq!(current_after, signing_keys::KeyResolution::Retired);
        assert!(matches!(
            successor_after,
            signing_keys::KeyResolution::Resolved(_)
        ));
    }

    /// A rotation whose successor signature does not match the current key's
    /// statement must be rejected rather than applied — otherwise anyone who
    /// merely possesses a *successor* key, without the current key's
    /// authorisation, could rotate a node away from its owner.
    #[tokio::test]
    async fn a_rotation_missing_the_current_keys_authorisation_is_rejected() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let now = SystemTime::now();
        let mut store = EnrollmentStore::connect(&database_url)
            .await
            .expect("test database connects");
        let current_key = SigningKey::from_bytes(&[13; 32]);
        let successor_key = SigningKey::from_bytes(&[14; 32]);
        let attacker_key = SigningKey::from_bytes(&[15; 32]);
        let signing_key_id = format!("signing-key-rotation-current-{}", node_suffix());
        let successor_key_id = format!("signing-key-rotation-successor-{}", node_suffix());
        let enrollment = activated_node(&mut store, &current_key, &signing_key_id, now).await;

        // Signed by an attacker's key instead of the node's actual current key.
        let rotation = signed_rotation(
            &attacker_key,
            &successor_key,
            KeyRotation {
                tenant_id: enrollment.tenant_id.clone(),
                repository_id: enrollment.repository_id.clone(),
                node_id: enrollment.proposed_node_id.clone(),
                current_key_id: signing_key_id.clone(),
                successor_key_id: successor_key_id.clone(),
                successor_public_key_fingerprint: public_key_fingerprint(
                    &successor_key.verifying_key().to_bytes(),
                ),
                successor_public_key: successor_key.verifying_key().to_bytes().to_vec(),
                current_key_signature: Vec::new(),
                successor_key_signature: Vec::new(),
                requested_overlap_seconds: 3_600,
            },
        );

        let result = store
            .rotate_key(&rotation, now)
            .await
            .expect("rejection is a normal result, not an error");

        assert_eq!(
            result.outcome,
            KeyRotationOutcome::Rejected(KeyRotationRejection::ContinuityProofInvalid)
        );

        let successor_after = signing_keys::resolve(
            &store.client,
            &signing_keys::EnvelopeBinding {
                signing_key_id: &successor_key_id,
                tenant_id: &rotation.tenant_id,
                repository_id: &rotation.repository_id,
                producer_id: &rotation.node_id,
                accepted_at: now,
            },
        )
        .await
        .expect("resolve queries succeed");
        assert_eq!(
            successor_after,
            signing_keys::KeyResolution::Unknown,
            "a rejected rotation must not register the successor key"
        );
    }

    #[tokio::test]
    async fn rotating_an_unknown_current_key_is_rejected_as_not_active() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let now = SystemTime::now();
        let mut store = EnrollmentStore::connect(&database_url)
            .await
            .expect("test database connects");
        let current_key = SigningKey::from_bytes(&[16; 32]);
        let successor_key = SigningKey::from_bytes(&[17; 32]);

        let rotation = signed_rotation(
            &current_key,
            &successor_key,
            KeyRotation {
                tenant_id: "tenant-test".to_owned(),
                repository_id: "repository-test".to_owned(),
                node_id: format!("node-{}", node_suffix()),
                current_key_id: format!("signing-key-never-registered-{}", node_suffix()),
                successor_key_id: format!("signing-key-rotation-successor-{}", node_suffix()),
                successor_public_key_fingerprint: public_key_fingerprint(
                    &successor_key.verifying_key().to_bytes(),
                ),
                successor_public_key: successor_key.verifying_key().to_bytes().to_vec(),
                current_key_signature: Vec::new(),
                successor_key_signature: Vec::new(),
                requested_overlap_seconds: 3_600,
            },
        );

        let result = store
            .rotate_key(&rotation, now)
            .await
            .expect("rejection is a normal result, not an error");

        assert_eq!(
            result.outcome,
            KeyRotationOutcome::Rejected(KeyRotationRejection::CurrentKeyNotActive)
        );
    }

    fn node_suffix() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos()
    }

    /// `activation_challenges.nonce` is globally unique, so every call to
    /// `activated_node` across every test needs its own nonce rather than a
    /// shared literal.
    fn nonce_for(seed: &str) -> [u8; 32] {
        let digest = Sha256::digest(seed.as_bytes());
        digest.into()
    }

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
