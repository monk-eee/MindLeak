//! Verifies that an EvidenceService request came from its enrolled node.
//!
//! Evidence has its own domain-separated signed bytes: a signature must bind
//! the task, kind, reference, digest, observed time, and agent session rather
//! than borrowing a claim or knowledge operation shape.

use std::time::SystemTime;

use ackplane_protocol::evidence_auth::EvidenceOperation;
use ackplane_protocol::v1;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::signing_keys::KeyResolution;

/// The signed-bytes contract is owned by the protocol crate so clients and
/// server cannot evolve different serializations of the same Evidence request.
pub use ackplane_protocol::evidence_auth::evidence_signing_bytes;

const MAX_EVIDENCE_AUTH_SKEW_SECS: i64 = 300;

/// Why an EvidenceService request was refused at its authentication boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceAuthRefusal {
    Unsigned,
    Unidentified,
    UnknownKey,
    BindingMismatch,
    KeyNotInForce,
    Revoked,
    BadSignature,
    MalformedTimestamp,
    StaleTimestamp,
    Replayed,
}

impl EvidenceAuthRefusal {
    pub fn is_authenticated_but_not_authorized(self) -> bool {
        matches!(self, Self::BindingMismatch)
    }

    pub fn diagnostic(self) -> &'static str {
        match self {
            Self::Unsigned => "this evidence request carried no authentication",
            Self::Unidentified => "authentication.signing_key_id is required",
            Self::UnknownKey => "signing_key_id names no key this authority holds",
            Self::BindingMismatch => {
                "that signing key is enrolled to a different tenant, repository or node"
            }
            Self::KeyNotInForce => {
                "the signing key is not currently in force: it is expired, retired, or not yet activated"
            }
            Self::Revoked => "the signing key has been revoked",
            Self::BadSignature => "the signature does not verify under the enrolled key",
            Self::MalformedTimestamp => "authentication.signed_at is not a valid RFC3339 timestamp",
            Self::StaleTimestamp => {
                "authentication.signed_at is outside the accepted clock-skew window"
            }
            Self::Replayed => {
                "this evidence authentication (signing_key_id, nonce) has already been used"
            }
        }
    }
}

/// Pure signature and freshness verification. Key lookup and nonce consumption
/// stay in EvidenceStore because they require the durable database connection.
pub fn verify(
    tenant_id: &str,
    repository_id: &str,
    operation: &EvidenceOperation,
    authentication: Option<&v1::EvidenceAuthentication>,
    resolution: &KeyResolution,
    now: SystemTime,
) -> Result<(), EvidenceAuthRefusal> {
    let Some(authentication) = authentication else {
        return Err(EvidenceAuthRefusal::Unsigned);
    };
    if authentication.signing_key_id.trim().is_empty() {
        return Err(EvidenceAuthRefusal::Unidentified);
    }
    if authentication.signature.is_empty() {
        return Err(EvidenceAuthRefusal::Unsigned);
    }
    check_freshness(&authentication.signed_at, now)?;

    let record = match resolution {
        KeyResolution::Resolved(record) => record,
        KeyResolution::Unknown => return Err(EvidenceAuthRefusal::UnknownKey),
        KeyResolution::BindingMismatch => return Err(EvidenceAuthRefusal::BindingMismatch),
        KeyResolution::Revoked => return Err(EvidenceAuthRefusal::Revoked),
        KeyResolution::NotYetActive | KeyResolution::Expired | KeyResolution::Retired => {
            return Err(EvidenceAuthRefusal::KeyNotInForce)
        }
    };

    let key = <&[u8; 32]>::try_from(record.public_key.as_slice())
        .ok()
        .and_then(|bytes| VerifyingKey::from_bytes(bytes).ok())
        .ok_or(EvidenceAuthRefusal::BadSignature)?;
    let signature = Signature::from_slice(&authentication.signature)
        .map_err(|_| EvidenceAuthRefusal::BadSignature)?;
    let bytes = evidence_signing_bytes(tenant_id, repository_id, operation, authentication);

    key.verify(&bytes, &signature)
        .map_err(|_| EvidenceAuthRefusal::BadSignature)
}

fn check_freshness(signed_at: &str, now: SystemTime) -> Result<(), EvidenceAuthRefusal> {
    let signed_at = OffsetDateTime::parse(signed_at, &Rfc3339)
        .map_err(|_| EvidenceAuthRefusal::MalformedTimestamp)?;
    let now = OffsetDateTime::from(now);
    if (signed_at - now).abs() > time::Duration::seconds(MAX_EVIDENCE_AUTH_SKEW_SECS) {
        return Err(EvidenceAuthRefusal::StaleTimestamp);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;
    use crate::signing_keys::SigningKeyRecord;

    const RECORD: EvidenceOperation<'static> = EvidenceOperation::Record {
        task_id: "task:123",
        evidence_kind: 1,
        source_ref: "commit:0123456789abcdef",
        content_digest: b"01234567890123456789012345678901",
        observed_at: "2026-01-01T00:00:00Z",
        reported_agent_session_id: "session:v1:agent",
        idempotency_key: "evidence:123",
    };

    fn fixed_now() -> SystemTime {
        OffsetDateTime::parse("2026-01-01T00:00:00Z", &Rfc3339)
            .unwrap()
            .into()
    }

    fn record(public_key: Vec<u8>) -> SigningKeyRecord {
        SigningKeyRecord {
            signing_key_id: "key-1".to_owned(),
            tenant_id: "tenant-a".to_owned(),
            repository_id: "repo-a".to_owned(),
            node_id: "node-1".to_owned(),
            public_key,
            public_key_fingerprint: "fingerprint".to_owned(),
            activated_at: SystemTime::UNIX_EPOCH,
            expires_at: None,
        }
    }

    fn signed_authentication(
        signing_key: &SigningKey,
        operation: &EvidenceOperation,
    ) -> v1::EvidenceAuthentication {
        let mut authentication = v1::EvidenceAuthentication {
            signing_key_id: "key-1".to_owned(),
            node_id: "node-1".to_owned(),
            signed_at: "2026-01-01T00:00:00Z".to_owned(),
            nonce: vec![7; 16],
            signature: Vec::new(),
        };
        let bytes = evidence_signing_bytes("tenant-a", "repo-a", operation, &authentication);
        authentication.signature = signing_key.sign(&bytes).to_bytes().to_vec();
        authentication
    }

    #[test]
    fn a_validly_signed_evidence_request_verifies() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, &RECORD);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                &RECORD,
                Some(&authentication),
                &resolution,
                fixed_now(),
            ),
            Ok(())
        );
    }

    #[test]
    fn a_signed_evidence_request_cannot_be_retargeted_to_another_task() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, &RECORD);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));
        let other_task = EvidenceOperation::Record {
            task_id: "task:456",
            evidence_kind: 1,
            source_ref: "commit:0123456789abcdef",
            content_digest: b"01234567890123456789012345678901",
            observed_at: "2026-01-01T00:00:00Z",
            reported_agent_session_id: "session:v1:agent",
            idempotency_key: "evidence:123",
        };

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                &other_task,
                Some(&authentication),
                &resolution,
                fixed_now(),
            ),
            Err(EvidenceAuthRefusal::BadSignature)
        );
    }

    #[test]
    fn a_binding_mismatch_is_authorization_not_authentication_failure() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, &RECORD);

        let refusal = verify(
            "tenant-a",
            "repo-a",
            &RECORD,
            Some(&authentication),
            &KeyResolution::BindingMismatch,
            fixed_now(),
        )
        .unwrap_err();

        assert_eq!(refusal, EvidenceAuthRefusal::BindingMismatch);
        assert!(refusal.is_authenticated_but_not_authorized());
    }

    #[test]
    fn evidence_domain_never_verifies_as_a_claim_domain() {
        let bytes = evidence_signing_bytes(
            "tenant-a",
            "repo-a",
            &RECORD,
            &signed_authentication(&SigningKey::from_bytes(&[9; 32]), &RECORD),
        );

        assert!(bytes.starts_with(ackplane_protocol::evidence_auth::EVIDENCE_DOMAIN));
        assert_ne!(
            ackplane_protocol::evidence_auth::EVIDENCE_DOMAIN,
            ackplane_protocol::claim_auth::CLAIM_DOMAIN
        );
    }
}
