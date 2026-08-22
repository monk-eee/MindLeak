//! Verifying that a `ConstitutionService` request came from the enrolled
//! node it names.
//!
//! Mirrors `knowledge_signature.rs`'s shape with its own refusal enum and
//! domain-separated signed bytes -- a constitution snapshot has no branch,
//! lease, or knowledge content to bind, so reusing another domain's refusal
//! type or signing bytes would sign nothing meaningful for this one.

use ackplane_protocol::constitution_auth::ConstitutionOperation;
use ackplane_protocol::v1;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::time::SystemTime;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::signing_keys::KeyResolution;

/// The signed-bytes contract lives in `ackplane-protocol` (the lowest layer
/// both this verifier and any future client-side signer depend on), so the
/// two sides can never construct incompatible serializations of the same
/// fields. Re-exported here so callers of this module need only one import.
pub use ackplane_protocol::constitution_auth::constitution_signing_bytes;

/// How far `authentication.signed_at` may drift from the verifier's clock, in
/// either direction, before a request is refused as stale rather than merely
/// old. Same bound as `claim_signature`/`knowledge_signature`'s own -- there
/// is nothing domain-specific about how much clock skew is tolerable.
const MAX_CONSTITUTION_AUTH_SKEW_SECS: i64 = 300;

/// Why a `ConstitutionService` request was refused at the trust boundary.
///
/// Structurally identical to `KnowledgeAuthRefusal`: a separate type rather
/// than a shared one, for the same reason ADR-0108 gives for not sharing
/// `ClaimAuthRefusal` with knowledge -- sharing it would invite the domains'
/// verification logic to be merged next, which is not this task's scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstitutionAuthRefusal {
    /// The request carried no `ConstitutionAuthentication` at all.
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
    /// request; the caller (not `verify`, which is pure) is the one that
    /// discovers this, since it needs the durable nonce store.
    Replayed,
}

impl ConstitutionAuthRefusal {
    /// A binding mismatch names a real key that is not this caller's; every
    /// other refusal means no caller was authenticated at all.
    pub fn is_authenticated_but_not_authorized(self) -> bool {
        matches!(self, Self::BindingMismatch)
    }

    pub fn diagnostic(self) -> &'static str {
        match self {
            Self::Unsigned => "this constitution request carried no authentication",
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
                "this constitution authentication (signing_key_id, nonce) has already been used"
            }
        }
    }
}

/// Verify a constitution request's authentication against its resolved key.
///
/// Pure: no database, no network. The lookup (`ConstitutionStore::resolve_signing_key`)
/// is the easy half; this is the half that decides whether the caller is who
/// it claims to be.
pub fn verify(
    tenant_id: &str,
    repository_id: &str,
    operation: &ConstitutionOperation,
    authentication: Option<&v1::ConstitutionAuthentication>,
    resolution: &KeyResolution,
    now: SystemTime,
) -> Result<(), ConstitutionAuthRefusal> {
    let Some(authentication) = authentication else {
        return Err(ConstitutionAuthRefusal::Unsigned);
    };
    if authentication.signing_key_id.trim().is_empty() {
        return Err(ConstitutionAuthRefusal::Unidentified);
    }
    if authentication.signature.is_empty() {
        return Err(ConstitutionAuthRefusal::Unsigned);
    }
    check_freshness(&authentication.signed_at, now)?;

    let record = match resolution {
        KeyResolution::Resolved(record) => record,
        KeyResolution::Unknown => return Err(ConstitutionAuthRefusal::UnknownKey),
        KeyResolution::BindingMismatch => return Err(ConstitutionAuthRefusal::BindingMismatch),
        KeyResolution::Revoked => return Err(ConstitutionAuthRefusal::Revoked),
        KeyResolution::NotYetActive | KeyResolution::Expired | KeyResolution::Retired => {
            return Err(ConstitutionAuthRefusal::KeyNotInForce)
        }
    };

    let key = <&[u8; 32]>::try_from(record.public_key.as_slice())
        .ok()
        .and_then(|bytes| VerifyingKey::from_bytes(bytes).ok())
        .ok_or(ConstitutionAuthRefusal::BadSignature)?;
    let signature = Signature::from_slice(&authentication.signature)
        .map_err(|_| ConstitutionAuthRefusal::BadSignature)?;
    let bytes = constitution_signing_bytes(tenant_id, repository_id, operation, authentication);

    key.verify(&bytes, &signature)
        .map_err(|_| ConstitutionAuthRefusal::BadSignature)
}

/// Pure clock-skew bound on `signed_at`, checked before any signature or
/// database work. A malformed timestamp and one merely too old/new are
/// different security stories: one is a protocol/client bug, the other is
/// what a captured-and-replayed request looks like before its nonce is even
/// considered.
fn check_freshness(signed_at: &str, now: SystemTime) -> Result<(), ConstitutionAuthRefusal> {
    let signed_at = OffsetDateTime::parse(signed_at, &Rfc3339)
        .map_err(|_| ConstitutionAuthRefusal::MalformedTimestamp)?;
    let now = OffsetDateTime::from(now);
    let skew = (signed_at - now).abs();
    if skew > time::Duration::seconds(MAX_CONSTITUTION_AUTH_SKEW_SECS) {
        return Err(ConstitutionAuthRefusal::StaleTimestamp);
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
        operation: &ConstitutionOperation,
    ) -> v1::ConstitutionAuthentication {
        let mut authentication = v1::ConstitutionAuthentication {
            signing_key_id: "key-1".to_owned(),
            node_id: node_id.to_owned(),
            signed_at: "2026-01-01T00:00:00Z".to_owned(),
            nonce: vec![7; 16],
            signature: Vec::new(),
        };
        let bytes = constitution_signing_bytes("tenant-a", "repo-a", operation, &authentication);
        authentication.signature = signing_key.sign(&bytes).to_bytes().to_vec();
        authentication
    }

    /// Every identity/timestamp/nonce test below authenticates a `GetActive`
    /// (the operation with no fields of its own) so those concerns stay
    /// decoupled from which operation is being authorized; the
    /// operation-binding test further down is what actually varies this.
    const GET_ACTIVE: ConstitutionOperation<'static> = ConstitutionOperation::GetActive;

    /// Matches `signed_authentication`'s hardcoded `signed_at`, so the
    /// freshness check every test must now also satisfy does not become a
    /// second, uncoordinated place to keep a timestamp in sync.
    fn fixed_now() -> SystemTime {
        OffsetDateTime::parse("2026-01-01T00:00:00Z", &Rfc3339)
            .unwrap()
            .into()
    }

    #[test]
    fn a_validly_signed_request_verifies() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", &GET_ACTIVE);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                &GET_ACTIVE,
                Some(&authentication),
                &resolution,
                fixed_now(),
            ),
            Ok(())
        );
    }

    #[test]
    fn an_unsigned_request_is_refused() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                &GET_ACTIVE,
                None,
                &resolution,
                fixed_now()
            ),
            Err(ConstitutionAuthRefusal::Unsigned)
        );
    }

    #[test]
    fn a_signature_from_an_unenrolled_key_is_refused() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", &GET_ACTIVE);

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                &GET_ACTIVE,
                Some(&authentication),
                &KeyResolution::Unknown,
                fixed_now(),
            ),
            Err(ConstitutionAuthRefusal::UnknownKey)
        );
    }

    #[test]
    fn a_binding_mismatch_is_refused_as_unauthorized_not_unauthenticated() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", &GET_ACTIVE);

        let refusal = verify(
            "tenant-a",
            "repo-a",
            &GET_ACTIVE,
            Some(&authentication),
            &KeyResolution::BindingMismatch,
            fixed_now(),
        )
        .unwrap_err();

        assert_eq!(refusal, ConstitutionAuthRefusal::BindingMismatch);
        assert!(refusal.is_authenticated_but_not_authorized());
    }

    #[test]
    fn a_revoked_key_is_refused() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", &GET_ACTIVE);

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                &GET_ACTIVE,
                Some(&authentication),
                &KeyResolution::Revoked,
                fixed_now(),
            ),
            Err(ConstitutionAuthRefusal::Revoked)
        );
    }

    #[test]
    fn a_signature_over_a_different_repository_does_not_verify() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", &GET_ACTIVE);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));

        // Same authentication, different repository_id: the signature was
        // never over this request's identity.
        assert_eq!(
            verify(
                "tenant-a",
                "repo-b",
                &GET_ACTIVE,
                Some(&authentication),
                &resolution,
                fixed_now(),
            ),
            Err(ConstitutionAuthRefusal::BadSignature)
        );
    }

    #[test]
    fn a_stale_timestamp_is_refused() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", &GET_ACTIVE);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));
        let too_late =
            fixed_now() + Duration::from_secs(MAX_CONSTITUTION_AUTH_SKEW_SECS as u64 + 1);

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                &GET_ACTIVE,
                Some(&authentication),
                &resolution,
                too_late,
            ),
            Err(ConstitutionAuthRefusal::StaleTimestamp)
        );
    }

    #[test]
    fn a_timestamp_too_far_in_the_past_is_refused() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", &GET_ACTIVE);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));
        let too_early =
            fixed_now() - Duration::from_secs(MAX_CONSTITUTION_AUTH_SKEW_SECS as u64 + 1);

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                &GET_ACTIVE,
                Some(&authentication),
                &resolution,
                too_early,
            ),
            Err(ConstitutionAuthRefusal::StaleTimestamp)
        );
    }

    #[test]
    fn an_unparseable_timestamp_is_refused() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let mut authentication = signed_authentication(&signing_key, "node-1", &GET_ACTIVE);
        authentication.signed_at = "not a timestamp".to_owned();
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                &GET_ACTIVE,
                Some(&authentication),
                &resolution,
                fixed_now(),
            ),
            Err(ConstitutionAuthRefusal::MalformedTimestamp)
        );
    }

    #[test]
    fn a_timestamp_just_inside_the_skew_window_still_verifies() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1", &GET_ACTIVE);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));
        let just_inside =
            fixed_now() + Duration::from_secs(MAX_CONSTITUTION_AUTH_SKEW_SECS as u64 - 1);

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                &GET_ACTIVE,
                Some(&authentication),
                &resolution,
                just_inside,
            ),
            Ok(())
        );
    }

    /// A signature is over one exact operation, not merely one identity. A
    /// `Publish` authentication does not verify as a `GetActive` even though
    /// every identity field, timestamp, and nonce is unchanged.
    #[test]
    fn a_signature_for_one_operation_does_not_verify_as_another() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let publish_op = ConstitutionOperation::Publish {
            version_id: "version-1",
            version: 4,
            status: "active",
            clause_count: 12,
        };
        let authentication = signed_authentication(&signing_key, "node-1", &publish_op);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                &GET_ACTIVE,
                Some(&authentication),
                &resolution,
                fixed_now(),
            ),
            Err(ConstitutionAuthRefusal::BadSignature)
        );
    }
}
