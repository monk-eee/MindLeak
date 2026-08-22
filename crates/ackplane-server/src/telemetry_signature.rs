//! Verifying that a `TelemetryService` request came from the enrolled node
//! it names.
//!
//! Mirrors `knowledge_signature.rs`'s shape with its own refusal enum and
//! domain-separated signed bytes -- telemetry has no equivalent of a claim's
//! branch/lease/paths or a knowledge statement's content, so reusing either
//! domain's authentication would sign nothing meaningful for this one.

use ackplane_protocol::telemetry_auth::TelemetryOperation;
use ackplane_protocol::v1;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::time::SystemTime;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::signing_keys::KeyResolution;

/// The signed-bytes contract lives in `ackplane-protocol`, so this verifier
/// and any future client-side signer can never construct incompatible
/// serializations of the same fields.
pub use ackplane_protocol::telemetry_auth::telemetry_signing_bytes;

/// Same bound as `knowledge_signature::MAX_KNOWLEDGE_AUTH_SKEW_SECS` --
/// nothing domain-specific about how much clock skew is tolerable.
const MAX_TELEMETRY_AUTH_SKEW_SECS: i64 = 300;

/// Why a `TelemetryService` request was refused at the trust boundary.
///
/// Structurally identical to `KnowledgeAuthRefusal` -- a separate type
/// rather than a shared one, so the two domains' verification logic is never
/// tempted to merge (the established precedent from ADR-0108's own rejected
/// alternatives).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryAuthRefusal {
    /// The request carried no `TelemetryAuthentication` at all.
    Unsigned,
    /// `signing_key_id` was empty.
    Unidentified,
    /// The named key is not one this authority holds.
    UnknownKey,
    /// The key exists but is bound to a different tenant, repository or node.
    BindingMismatch,
    /// The key's authority had not begun, or had ended by expiry or
    /// retirement, when this arrived.
    KeyNotInForce,
    /// An administrator or incident workflow revoked this key.
    Revoked,
    /// The bytes do not verify under the resolved key.
    BadSignature,
    /// `signed_at` is not a parseable RFC3339 timestamp.
    MalformedTimestamp,
    /// `signed_at` is outside the bounded clock-skew window around now.
    StaleTimestamp,
    /// This exact (signing_key_id, nonce) pair already authenticated a
    /// request; the caller (not `verify`, which is pure) discovers this,
    /// since it needs the durable nonce store.
    Replayed,
}

impl TelemetryAuthRefusal {
    /// A binding mismatch names a real key that is not this caller's; every
    /// other refusal means no caller was authenticated at all.
    pub fn is_authenticated_but_not_authorized(self) -> bool {
        matches!(self, Self::BindingMismatch)
    }

    pub fn diagnostic(self) -> &'static str {
        match self {
            Self::Unsigned => "this telemetry request carried no authentication",
            Self::Unidentified => "authentication.signing_key_id is required",
            Self::UnknownKey => "signing_key_id names no key this authority holds",
            Self::BindingMismatch => {
                "that signing key is enrolled to a different tenant, repository or node"
            }
            Self::KeyNotInForce => {
                "the signing key is not currently in force: it is expired, retired, or not yet \
                 activated"
            }
            Self::Revoked => "the signing key has been revoked",
            Self::BadSignature => "the signature does not verify under the enrolled key",
            Self::MalformedTimestamp => "authentication.signed_at is not a valid RFC3339 timestamp",
            Self::StaleTimestamp => {
                "authentication.signed_at is outside the accepted clock-skew window"
            }
            Self::Replayed => {
                "this telemetry authentication (signing_key_id, nonce) has already been used"
            }
        }
    }
}

/// Verify a telemetry request's authentication against its resolved key.
///
/// Pure: no database, no network. The lookup
/// (`TelemetryStore::resolve_signing_key`) is the easy half; this is the
/// half that decides whether the caller is who it claims to be.
pub fn verify(
    tenant_id: &str,
    repository_id: &str,
    operation: &TelemetryOperation,
    authentication: Option<&v1::TelemetryAuthentication>,
    resolution: &KeyResolution,
    now: SystemTime,
) -> Result<(), TelemetryAuthRefusal> {
    let Some(authentication) = authentication else {
        return Err(TelemetryAuthRefusal::Unsigned);
    };
    if authentication.signing_key_id.trim().is_empty() {
        return Err(TelemetryAuthRefusal::Unidentified);
    }
    if authentication.signature.is_empty() {
        return Err(TelemetryAuthRefusal::Unsigned);
    }
    check_freshness(&authentication.signed_at, now)?;

    let record = match resolution {
        KeyResolution::Resolved(record) => record,
        KeyResolution::Unknown => return Err(TelemetryAuthRefusal::UnknownKey),
        KeyResolution::BindingMismatch => return Err(TelemetryAuthRefusal::BindingMismatch),
        KeyResolution::Revoked => return Err(TelemetryAuthRefusal::Revoked),
        KeyResolution::NotYetActive | KeyResolution::Expired | KeyResolution::Retired => {
            return Err(TelemetryAuthRefusal::KeyNotInForce)
        }
    };

    let key = <&[u8; 32]>::try_from(record.public_key.as_slice())
        .ok()
        .and_then(|bytes| VerifyingKey::from_bytes(bytes).ok())
        .ok_or(TelemetryAuthRefusal::BadSignature)?;
    let signature = Signature::from_slice(&authentication.signature)
        .map_err(|_| TelemetryAuthRefusal::BadSignature)?;
    let bytes = telemetry_signing_bytes(tenant_id, repository_id, operation, authentication);

    key.verify(&bytes, &signature)
        .map_err(|_| TelemetryAuthRefusal::BadSignature)
}

/// Pure clock-skew bound on `signed_at`, checked before any signature or
/// database work.
fn check_freshness(signed_at: &str, now: SystemTime) -> Result<(), TelemetryAuthRefusal> {
    let signed_at = OffsetDateTime::parse(signed_at, &Rfc3339)
        .map_err(|_| TelemetryAuthRefusal::MalformedTimestamp)?;
    let now = OffsetDateTime::from(now);
    let skew = (signed_at - now).abs();
    if skew > time::Duration::seconds(MAX_TELEMETRY_AUTH_SKEW_SECS) {
        return Err(TelemetryAuthRefusal::StaleTimestamp);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use ed25519_dalek::{Signer, SigningKey};

    use super::*;
    use crate::signing_keys::SigningKeyRecord;

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
        node_id: &str,
        operation: &TelemetryOperation,
    ) -> v1::TelemetryAuthentication {
        let mut authentication = v1::TelemetryAuthentication {
            signing_key_id: "key-1".to_owned(),
            node_id: node_id.to_owned(),
            signed_at: "2026-01-01T00:00:00Z".to_owned(),
            nonce: vec![7; 16],
            signature: Vec::new(),
        };
        let bytes = telemetry_signing_bytes("tenant-a", "repo-a", operation, &authentication);
        authentication.signature = signing_key.sign(&bytes).to_bytes().to_vec();
        authentication
    }

    const READ: TelemetryOperation<'static> = TelemetryOperation::Read {
        kind: 0,
        name: None,
        bucket_seconds: 3600,
        max_points: 60,
    };

    fn fixed_now() -> SystemTime {
        OffsetDateTime::parse("2026-01-01T00:00:00Z", &Rfc3339)
            .unwrap()
            .into()
    }

    #[test]
    fn a_validly_signed_request_verifies() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", &READ);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                &READ,
                Some(&authentication),
                &resolution,
                fixed_now(),
            ),
            Ok(())
        );
    }

    #[test]
    fn an_unsigned_request_is_refused() {
        let resolution = KeyResolution::Resolved(record(vec![0; 32]));
        assert_eq!(
            verify("tenant-a", "repo-a", &READ, None, &resolution, fixed_now()),
            Err(TelemetryAuthRefusal::Unsigned)
        );
    }

    #[test]
    fn a_signature_from_the_wrong_key_is_refused() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let other_key = SigningKey::from_bytes(&[3; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", &READ);
        let resolution =
            KeyResolution::Resolved(record(other_key.verifying_key().to_bytes().to_vec()));

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                &READ,
                Some(&authentication),
                &resolution,
                fixed_now(),
            ),
            Err(TelemetryAuthRefusal::BadSignature)
        );
    }

    /// The signature must bind the exact operation: reusing a signature
    /// minted over a `Read` to authorize a `Record` must not verify.
    #[test]
    fn a_signature_does_not_transfer_across_operations() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", &READ);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));
        let different_operation = TelemetryOperation::Read {
            kind: 0,
            name: None,
            bucket_seconds: 60,
            max_points: 60,
        };

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                &different_operation,
                Some(&authentication),
                &resolution,
                fixed_now(),
            ),
            Err(TelemetryAuthRefusal::BadSignature)
        );
    }

    #[test]
    fn a_stale_timestamp_is_refused_before_the_signature_is_even_checked() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", &READ);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));
        let far_future = fixed_now() + Duration::from_secs(3600);

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                &READ,
                Some(&authentication),
                &resolution,
                far_future,
            ),
            Err(TelemetryAuthRefusal::StaleTimestamp)
        );
    }

    #[test]
    fn a_revoked_key_is_refused() {
        let authentication = v1::TelemetryAuthentication {
            signing_key_id: "key-1".to_owned(),
            node_id: "node-1".to_owned(),
            signed_at: "2026-01-01T00:00:00Z".to_owned(),
            nonce: vec![7; 16],
            signature: vec![1; 64],
        };

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                &READ,
                Some(&authentication),
                &KeyResolution::Revoked,
                fixed_now(),
            ),
            Err(TelemetryAuthRefusal::Revoked)
        );
    }

    #[test]
    fn a_binding_mismatch_is_authenticated_but_not_authorized() {
        assert!(TelemetryAuthRefusal::BindingMismatch.is_authenticated_but_not_authorized());
        assert!(!TelemetryAuthRefusal::Unsigned.is_authenticated_but_not_authorized());
    }
}
