//! Verifying that a claim request came from the enrolled node it names.
//!
//! Before this, `ClaimDelegationService` accepted `DelegateClaim`/
//! `ReleaseClaim`/`RenewClaim`/`RecoverClaim` from any caller naming any
//! `tenant_id`/`repository_id`/`owner_id` -- nothing tied the request to an
//! enrolled node's key. This reuses the exact `signing_keys` resolution and
//! Ed25519 verification `envelope_signature` already established, with its own
//! domain string, so a signature captured here can never be replayed as an
//! envelope or a connection challenge response.

use ackplane_protocol::v1;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::time::SystemTime;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::signing_keys::KeyResolution;

/// Domain separation for claim-request signatures.
const CLAIM_DOMAIN: &[u8] = b"mindleak.ackplane.v1.claim\0";

/// How far `authentication.signed_at` may drift from the verifier's clock, in
/// either direction, before a request is refused as stale rather than merely
/// old. Bounding it is what makes a captured signature stop being usable
/// after a while, rather than only after its (unenforced, until this) nonce
/// happens to collide.
const MAX_CLAIM_AUTH_SKEW_SECS: i64 = 300;

/// The exact bytes a node signs to authenticate a claim request.
///
/// Binds the authentication to this specific claim's identity -- tenant,
/// repository, task, and the owner it is requesting on behalf of -- so a
/// signature valid for one claim can never verify against another, even from
/// the same key. Every field is length-delimited, following
/// `envelope_signature::envelope_signing_bytes`.
pub fn claim_signing_bytes(
    tenant_id: &str,
    repository_id: &str,
    task_id: &str,
    owner_id: &str,
    authentication: &v1::ClaimAuthentication,
) -> Vec<u8> {
    let fields: [&[u8]; 8] = [
        authentication.signing_key_id.as_bytes(),
        authentication.node_id.as_bytes(),
        authentication.signed_at.as_bytes(),
        &authentication.nonce,
        tenant_id.as_bytes(),
        repository_id.as_bytes(),
        task_id.as_bytes(),
        owner_id.as_bytes(),
    ];

    let mut bytes = Vec::with_capacity(
        CLAIM_DOMAIN.len() + fields.iter().map(|field| 4 + field.len()).sum::<usize>(),
    );
    bytes.extend_from_slice(CLAIM_DOMAIN);
    for field in fields {
        bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
        bytes.extend_from_slice(field);
    }
    bytes
}

/// Why a claim request was refused at the trust boundary.
///
/// Separate variants rather than one generic "unauthenticated", because "no
/// signature was sent", "we hold no such key" and "that key is not yours" are
/// different security stories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimAuthRefusal {
    /// The request carried no `ClaimAuthentication` at all.
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

impl ClaimAuthRefusal {
    /// A binding mismatch names a real key that is not this caller's; every
    /// other refusal means no caller was authenticated at all.
    pub fn is_authenticated_but_not_authorized(self) -> bool {
        matches!(self, Self::BindingMismatch)
    }

    pub fn diagnostic(self) -> &'static str {
        match self {
            Self::Unsigned => "this claim request carried no authentication",
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
                "this claim authentication (signing_key_id, nonce) has already been used"
            }
        }
    }
}

/// Verify a claim request's authentication against its resolved key.
///
/// Pure: no database, no network. The lookup (`ClaimStore::resolve_signing_key`)
/// is the easy half; this is the half that decides whether the caller is who
/// it claims to be.
pub fn verify(
    tenant_id: &str,
    repository_id: &str,
    task_id: &str,
    owner_id: &str,
    authentication: Option<&v1::ClaimAuthentication>,
    resolution: &KeyResolution,
    now: SystemTime,
) -> Result<(), ClaimAuthRefusal> {
    let Some(authentication) = authentication else {
        return Err(ClaimAuthRefusal::Unsigned);
    };
    if authentication.signing_key_id.trim().is_empty() {
        return Err(ClaimAuthRefusal::Unidentified);
    }
    if authentication.signature.is_empty() {
        return Err(ClaimAuthRefusal::Unsigned);
    }
    check_freshness(&authentication.signed_at, now)?;

    let record = match resolution {
        KeyResolution::Resolved(record) => record,
        KeyResolution::Unknown => return Err(ClaimAuthRefusal::UnknownKey),
        KeyResolution::BindingMismatch => return Err(ClaimAuthRefusal::BindingMismatch),
        KeyResolution::Revoked => return Err(ClaimAuthRefusal::Revoked),
        KeyResolution::NotYetActive | KeyResolution::Expired | KeyResolution::Retired => {
            return Err(ClaimAuthRefusal::KeyNotInForce)
        }
    };

    let key = <&[u8; 32]>::try_from(record.public_key.as_slice())
        .ok()
        .and_then(|bytes| VerifyingKey::from_bytes(bytes).ok())
        .ok_or(ClaimAuthRefusal::BadSignature)?;
    let signature = Signature::from_slice(&authentication.signature)
        .map_err(|_| ClaimAuthRefusal::BadSignature)?;
    let bytes = claim_signing_bytes(tenant_id, repository_id, task_id, owner_id, authentication);

    key.verify(&bytes, &signature)
        .map_err(|_| ClaimAuthRefusal::BadSignature)
}

/// Pure clock-skew bound on `signed_at`, checked before any signature or
/// database work. A malformed timestamp and one merely too old/new are
/// different security stories: one is a protocol/client bug, the other is
/// what a captured-and-replayed request looks like before its nonce is even
/// considered.
fn check_freshness(signed_at: &str, now: SystemTime) -> Result<(), ClaimAuthRefusal> {
    let signed_at = OffsetDateTime::parse(signed_at, &Rfc3339)
        .map_err(|_| ClaimAuthRefusal::MalformedTimestamp)?;
    let now = OffsetDateTime::from(now);
    let skew = (signed_at - now).abs();
    if skew > time::Duration::seconds(MAX_CLAIM_AUTH_SKEW_SECS) {
        return Err(ClaimAuthRefusal::StaleTimestamp);
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

    fn signed_authentication(signing_key: &SigningKey, node_id: &str) -> v1::ClaimAuthentication {
        let mut authentication = v1::ClaimAuthentication {
            signing_key_id: "key-1".to_owned(),
            node_id: node_id.to_owned(),
            signed_at: "2026-01-01T00:00:00Z".to_owned(),
            nonce: vec![7; 16],
            signature: Vec::new(),
        };
        let bytes = claim_signing_bytes("tenant-a", "repo-a", "task-1", "owner-1", &authentication);
        authentication.signature = signing_key.sign(&bytes).to_bytes().to_vec();
        authentication
    }

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
        let authentication = signed_authentication(&signing_key, "node-1");
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-1",
                "owner-1",
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
                "task-1",
                "owner-1",
                None,
                &resolution,
                fixed_now(),
            ),
            Err(ClaimAuthRefusal::Unsigned)
        );
    }

    #[test]
    fn a_signature_from_an_unenrolled_key_is_refused() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1");

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-1",
                "owner-1",
                Some(&authentication),
                &KeyResolution::Unknown,
                fixed_now(),
            ),
            Err(ClaimAuthRefusal::UnknownKey)
        );
    }

    #[test]
    fn a_binding_mismatch_is_refused_as_unauthorized_not_unauthenticated() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1");

        let refusal = verify(
            "tenant-a",
            "repo-a",
            "task-1",
            "owner-1",
            Some(&authentication),
            &KeyResolution::BindingMismatch,
            fixed_now(),
        )
        .unwrap_err();

        assert_eq!(refusal, ClaimAuthRefusal::BindingMismatch);
        assert!(refusal.is_authenticated_but_not_authorized());
    }

    #[test]
    fn a_revoked_key_is_refused() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1");

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-1",
                "owner-1",
                Some(&authentication),
                &KeyResolution::Revoked,
                fixed_now(),
            ),
            Err(ClaimAuthRefusal::Revoked)
        );
    }

    #[test]
    fn a_signature_over_a_different_claim_does_not_verify() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1");
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));

        // Same authentication, different task_id: the signature was never over
        // this claim's identity.
        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-2",
                "owner-1",
                Some(&authentication),
                &resolution,
                fixed_now(),
            ),
            Err(ClaimAuthRefusal::BadSignature)
        );
    }

    #[test]
    fn a_stale_timestamp_is_refused() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1");
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));
        let too_late = fixed_now() + Duration::from_secs(MAX_CLAIM_AUTH_SKEW_SECS as u64 + 1);

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-1",
                "owner-1",
                Some(&authentication),
                &resolution,
                too_late,
            ),
            Err(ClaimAuthRefusal::StaleTimestamp)
        );
    }

    #[test]
    fn a_timestamp_too_far_in_the_future_is_refused() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1");
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));
        let too_early = fixed_now() - Duration::from_secs(MAX_CLAIM_AUTH_SKEW_SECS as u64 + 1);

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-1",
                "owner-1",
                Some(&authentication),
                &resolution,
                too_early,
            ),
            Err(ClaimAuthRefusal::StaleTimestamp)
        );
    }

    #[test]
    fn an_unparseable_timestamp_is_refused() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let mut authentication = signed_authentication(&signing_key, "node-1");
        authentication.signed_at = "not a timestamp".to_owned();
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-1",
                "owner-1",
                Some(&authentication),
                &resolution,
                fixed_now(),
            ),
            Err(ClaimAuthRefusal::MalformedTimestamp)
        );
    }

    #[test]
    fn a_timestamp_just_inside_the_skew_window_still_verifies() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let authentication = signed_authentication(&signing_key, "node-1");
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));
        let just_inside = fixed_now() + Duration::from_secs(MAX_CLAIM_AUTH_SKEW_SECS as u64 - 1);

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-1",
                "owner-1",
                Some(&authentication),
                &resolution,
                just_inside,
            ),
            Ok(())
        );
    }
}
