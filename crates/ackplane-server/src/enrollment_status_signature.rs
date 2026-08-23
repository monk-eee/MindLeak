//! Verifying that a `CheckEnrollmentStatus` request came from the holder of
//! the exact key it names (ADR-0122).
//!
//! Deliberately narrower than `knowledge_signature.rs`'s `verify()`: this
//! domain has no `signing_key_id` to resolve against a registry, because a
//! candidate binding may not be registered at all yet. The caller looks up
//! the stored public key for the claimed (tenant, repository, node,
//! fingerprint) tuple first (an absent binding never reaches this function);
//! `verify()` only proves whether the presented signature matches that key.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use ackplane_protocol::enrollment_status_auth::{
    enrollment_status_signing_bytes, EnrollmentStatusOperation,
};
use ackplane_protocol::v1;

/// How far `authentication.signed_at` may drift from the verifier's clock, in
/// either direction, before a request is refused as stale. Same bound as
/// every other domain's `MAX_*_AUTH_SKEW_SECS` -- there is nothing
/// domain-specific about how much clock skew is tolerable.
const MAX_ENROLLMENT_STATUS_AUTH_SKEW_SECS: i64 = 300;

/// Why a `CheckEnrollmentStatus` request's proof of possession did not
/// verify. The caller collapses every variant to the identical generic
/// "not enrolled" result (ADR-0122 decision 5) -- this type exists so the
/// server can log and test each cause distinctly, not so a client ever sees
/// which one fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrollmentStatusAuthRefusal {
    /// The request carried no `EnrollmentStatusAuthentication` at all.
    Unsigned,
    /// `signed_at` is not a parseable RFC3339 timestamp.
    MalformedTimestamp,
    /// `signed_at` is outside the bounded clock-skew window around now.
    StaleTimestamp,
    /// The stored public key is malformed, or the bytes do not verify under
    /// it -- collapsed into one variant because a caller that fails either
    /// way holds no valid proof, and distinguishing them would let a caller
    /// learn something about the stored record from *how* verification
    /// failed.
    BadSignature,
}

impl EnrollmentStatusAuthRefusal {
    pub fn diagnostic(self) -> &'static str {
        match self {
            Self::Unsigned => "this enrollment-status request carried no authentication",
            Self::MalformedTimestamp => "authentication.signed_at is not a valid RFC3339 timestamp",
            Self::StaleTimestamp => {
                "authentication.signed_at is outside the accepted clock-skew window"
            }
            Self::BadSignature => "the signature does not verify under the candidate's stored key",
        }
    }
}

/// Verify a `CheckEnrollmentStatus` request's proof of possession against the
/// public key already found for its claimed binding.
///
/// Pure: no database, no network. Nonce replay-checking is a separate,
/// stateful concern the caller performs after this succeeds (mirroring
/// `knowledge_signature::verify`'s split), because a forged request must
/// never be able to burn a legitimate nonce out from under its owner.
pub fn verify(
    tenant_id: &str,
    repository_id: &str,
    candidate_public_key: &[u8],
    authentication: Option<&v1::EnrollmentStatusAuthentication>,
    now: std::time::SystemTime,
) -> Result<(), EnrollmentStatusAuthRefusal> {
    let Some(authentication) = authentication else {
        return Err(EnrollmentStatusAuthRefusal::Unsigned);
    };
    if authentication.signature.is_empty() {
        return Err(EnrollmentStatusAuthRefusal::Unsigned);
    }
    check_freshness(&authentication.signed_at, now)?;

    let key = <&[u8; 32]>::try_from(candidate_public_key)
        .ok()
        .and_then(|bytes| VerifyingKey::from_bytes(bytes).ok())
        .ok_or(EnrollmentStatusAuthRefusal::BadSignature)?;
    let signature = Signature::from_slice(&authentication.signature)
        .map_err(|_| EnrollmentStatusAuthRefusal::BadSignature)?;
    let bytes = enrollment_status_signing_bytes(
        tenant_id,
        repository_id,
        EnrollmentStatusOperation::Check,
        authentication,
    );

    key.verify(&bytes, &signature)
        .map_err(|_| EnrollmentStatusAuthRefusal::BadSignature)
}

/// Pure clock-skew bound on `signed_at`, checked before any signature work.
/// A malformed timestamp and one merely too old/new are different security
/// stories: one is a protocol/client bug, the other is what a
/// captured-and-replayed request looks like before its nonce is even
/// considered.
fn check_freshness(
    signed_at: &str,
    now: std::time::SystemTime,
) -> Result<(), EnrollmentStatusAuthRefusal> {
    let signed_at = OffsetDateTime::parse(signed_at, &Rfc3339)
        .map_err(|_| EnrollmentStatusAuthRefusal::MalformedTimestamp)?;
    let now = OffsetDateTime::from(now);
    let skew = (signed_at - now).abs();
    if skew > time::Duration::seconds(MAX_ENROLLMENT_STATUS_AUTH_SKEW_SECS) {
        return Err(EnrollmentStatusAuthRefusal::StaleTimestamp);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    fn signed_authentication(
        signing_key: &SigningKey,
        node_id: &str,
        key_fingerprint: &str,
    ) -> v1::EnrollmentStatusAuthentication {
        let mut authentication = v1::EnrollmentStatusAuthentication {
            node_id: node_id.to_owned(),
            key_fingerprint: key_fingerprint.to_owned(),
            signed_at: "2026-01-01T00:00:00Z".to_owned(),
            nonce: vec![7; 16],
            signature: Vec::new(),
        };
        let bytes = enrollment_status_signing_bytes(
            "tenant-a",
            "repo-a",
            EnrollmentStatusOperation::Check,
            &authentication,
        );
        authentication.signature = signing_key.sign(&bytes).to_bytes().to_vec();
        authentication
    }

    fn now() -> SystemTime {
        OffsetDateTime::parse("2026-01-01T00:00:00Z", &Rfc3339)
            .unwrap()
            .into()
    }

    #[test]
    fn a_validly_signed_request_verifies() {
        let signing_key = SigningKey::from_bytes(&[9u8; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", "fingerprint-1");
        let result = verify(
            "tenant-a",
            "repo-a",
            signing_key.verifying_key().as_bytes(),
            Some(&authentication),
            now(),
        );
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn an_unsigned_request_is_refused() {
        let result = verify("tenant-a", "repo-a", &[9u8; 32], None, now());
        assert_eq!(result, Err(EnrollmentStatusAuthRefusal::Unsigned));
    }

    #[test]
    fn an_empty_signature_is_refused_as_unsigned() {
        let signing_key = SigningKey::from_bytes(&[9u8; 32]);
        let mut authentication = signed_authentication(&signing_key, "node-1", "fingerprint-1");
        authentication.signature.clear();
        let result = verify(
            "tenant-a",
            "repo-a",
            signing_key.verifying_key().as_bytes(),
            Some(&authentication),
            now(),
        );
        assert_eq!(result, Err(EnrollmentStatusAuthRefusal::Unsigned));
    }

    #[test]
    fn an_unparseable_timestamp_is_refused() {
        let signing_key = SigningKey::from_bytes(&[9u8; 32]);
        let mut authentication = signed_authentication(&signing_key, "node-1", "fingerprint-1");
        authentication.signed_at = "not-a-timestamp".to_owned();
        let result = verify(
            "tenant-a",
            "repo-a",
            signing_key.verifying_key().as_bytes(),
            Some(&authentication),
            now(),
        );
        assert_eq!(result, Err(EnrollmentStatusAuthRefusal::MalformedTimestamp));
    }

    #[test]
    fn a_stale_timestamp_is_refused() {
        let signing_key = SigningKey::from_bytes(&[9u8; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", "fingerprint-1");
        let far_future = now() + Duration::from_secs(3600);
        let result = verify(
            "tenant-a",
            "repo-a",
            signing_key.verifying_key().as_bytes(),
            Some(&authentication),
            far_future,
        );
        assert_eq!(result, Err(EnrollmentStatusAuthRefusal::StaleTimestamp));
    }

    #[test]
    fn a_timestamp_just_inside_the_skew_window_still_verifies() {
        let signing_key = SigningKey::from_bytes(&[9u8; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", "fingerprint-1");
        let just_inside = now() + Duration::from_secs(299);
        let result = verify(
            "tenant-a",
            "repo-a",
            signing_key.verifying_key().as_bytes(),
            Some(&authentication),
            just_inside,
        );
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn a_signature_from_a_different_key_does_not_verify() {
        let signing_key = SigningKey::from_bytes(&[9u8; 32]);
        let other_key = SigningKey::from_bytes(&[3u8; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", "fingerprint-1");
        let result = verify(
            "tenant-a",
            "repo-a",
            other_key.verifying_key().as_bytes(),
            Some(&authentication),
            now(),
        );
        assert_eq!(result, Err(EnrollmentStatusAuthRefusal::BadSignature));
    }

    #[test]
    fn a_signature_over_a_different_tenant_does_not_verify() {
        let signing_key = SigningKey::from_bytes(&[9u8; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", "fingerprint-1");
        let result = verify(
            "tenant-b",
            "repo-a",
            signing_key.verifying_key().as_bytes(),
            Some(&authentication),
            now(),
        );
        assert_eq!(result, Err(EnrollmentStatusAuthRefusal::BadSignature));
    }

    #[test]
    fn a_malformed_stored_key_is_refused_as_bad_signature() {
        let signing_key = SigningKey::from_bytes(&[9u8; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", "fingerprint-1");
        let result = verify(
            "tenant-a",
            "repo-a",
            &[1, 2, 3],
            Some(&authentication),
            now(),
        );
        assert_eq!(result, Err(EnrollmentStatusAuthRefusal::BadSignature));
    }
}
