//! Verifying that a claim request came from the enrolled node it names.
//!
//! Before this, `ClaimDelegationService` accepted `DelegateClaim`/
//! `ReleaseClaim`/`RenewClaim`/`RecoverClaim` from any caller naming any
//! `tenant_id`/`repository_id`/`owner_id` -- nothing tied the request to an
//! enrolled node's key. This reuses the exact `signing_keys` resolution and
//! Ed25519 verification `envelope_signature` already established, with its own
//! domain string, so a signature captured here can never be replayed as an
//! envelope or a connection challenge response.

use ackplane_protocol::claim_auth::ClaimOperation;
use ackplane_protocol::v1;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::time::SystemTime;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::signing_keys::KeyResolution;

/// The signed-bytes contract lives in `ackplane-protocol` (the lowest layer
/// both this verifier and `ackplane-client`'s request signer depend on), so
/// the two sides can never construct incompatible serializations of the same
/// fields. Re-exported here so existing callers of this module are unaffected.
pub use ackplane_protocol::claim_auth::claim_signing_bytes;

/// How far `authentication.signed_at` may drift from the verifier's clock, in
/// either direction, before a request is refused as stale rather than merely
/// old. Bounding it is what makes a captured signature stop being usable
/// after a while, rather than only after its (unenforced, until this) nonce
/// happens to collide.
const MAX_CLAIM_AUTH_SKEW_SECS: i64 = 300;

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
#[allow(clippy::too_many_arguments)]
pub fn verify(
    tenant_id: &str,
    repository_id: &str,
    task_id: &str,
    owner_id: &str,
    operation: &ClaimOperation,
    authentication: Option<&v1::ClaimAuthentication>,
    resolution: &KeyResolution,
    now: SystemTime,
) -> Result<(), ClaimAuthRefusal> {
    let Some(authentication) = authentication else {
        return Err(ClaimAuthRefusal::Unsigned);
    };
    let bytes = claim_signing_bytes(
        tenant_id,
        repository_id,
        task_id,
        owner_id,
        operation,
        authentication,
    );
    verify_signed_bytes(authentication, resolution, &bytes, now)
}

/// Verify one operation's already-domain-separated signed bytes against an
/// enrolled key. The caller owns the operation-specific byte contract; this
/// shared boundary owns key lifecycle, timestamp, and Ed25519 checks.
pub fn verify_signed_bytes(
    authentication: &v1::ClaimAuthentication,
    resolution: &KeyResolution,
    signed_bytes: &[u8],
    now: SystemTime,
) -> Result<(), ClaimAuthRefusal> {
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

    key.verify(signed_bytes, &signature)
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

    fn signed_authentication(
        signing_key: &SigningKey,
        node_id: &str,
        operation: &ClaimOperation,
    ) -> v1::ClaimAuthentication {
        let mut authentication = v1::ClaimAuthentication {
            signing_key_id: "key-1".to_owned(),
            node_id: node_id.to_owned(),
            signed_at: "2026-01-01T00:00:00Z".to_owned(),
            nonce: vec![7; 16],
            signature: Vec::new(),
        };
        let bytes = claim_signing_bytes(
            "tenant-a",
            "repo-a",
            "task-1",
            "owner-1",
            operation,
            &authentication,
        );
        authentication.signature = signing_key.sign(&bytes).to_bytes().to_vec();
        authentication
    }

    /// Every identity/timestamp/nonce test below authenticates a `Release`
    /// (no operation-specific fields) so those concerns stay decoupled from
    /// which operation is being authorized; the operation-binding tests
    /// further down are what actually vary this.
    const RELEASE: ClaimOperation<'static> = ClaimOperation::Release;

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
        let authentication = signed_authentication(&signing_key, "node-1", &RELEASE);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-1",
                "owner-1",
                &RELEASE,
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
                &RELEASE,
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
        let authentication = signed_authentication(&signing_key, "node-1", &RELEASE);

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-1",
                "owner-1",
                &RELEASE,
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
        let authentication = signed_authentication(&signing_key, "node-1", &RELEASE);

        let refusal = verify(
            "tenant-a",
            "repo-a",
            "task-1",
            "owner-1",
            &RELEASE,
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
        let authentication = signed_authentication(&signing_key, "node-1", &RELEASE);

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-1",
                "owner-1",
                &RELEASE,
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
        let authentication = signed_authentication(&signing_key, "node-1", &RELEASE);
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
                &RELEASE,
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
        let authentication = signed_authentication(&signing_key, "node-1", &RELEASE);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));
        let too_late = fixed_now() + Duration::from_secs(MAX_CLAIM_AUTH_SKEW_SECS as u64 + 1);

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-1",
                "owner-1",
                &RELEASE,
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
        let authentication = signed_authentication(&signing_key, "node-1", &RELEASE);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));
        let too_early = fixed_now() - Duration::from_secs(MAX_CLAIM_AUTH_SKEW_SECS as u64 + 1);

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-1",
                "owner-1",
                &RELEASE,
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
        let mut authentication = signed_authentication(&signing_key, "node-1", &RELEASE);
        authentication.signed_at = "not a timestamp".to_owned();
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-1",
                "owner-1",
                &RELEASE,
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
        let authentication = signed_authentication(&signing_key, "node-1", &RELEASE);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));
        let just_inside = fixed_now() + Duration::from_secs(MAX_CLAIM_AUTH_SKEW_SECS as u64 - 1);

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-1",
                "owner-1",
                &RELEASE,
                Some(&authentication),
                &resolution,
                just_inside,
            ),
            Ok(())
        );
    }

    /// ADR-0100 decision 10/12: a signature is over one exact operation, not
    /// merely one identity. A `Delegate` authentication does not verify as a
    /// `Renew` even though every identity field, timestamp, and nonce is
    /// unchanged.
    #[test]
    fn a_signature_for_one_operation_does_not_verify_as_another() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let delegate = ClaimOperation::Delegate {
            branch: "feat/x",
            lease_seconds: 60,
            paths: &[],
            symbols: &[],
        };
        let authentication = signed_authentication(&signing_key, "node-1", &delegate);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));

        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-1",
                "owner-1",
                &ClaimOperation::Renew { lease_seconds: 60 },
                Some(&authentication),
                &resolution,
                fixed_now(),
            ),
            Err(ClaimAuthRefusal::BadSignature)
        );
    }

    /// A `Delegate` authentication signed for a 60-second lease does not
    /// verify for the identical request with a 120-second lease: the
    /// operation's own fields are bound, not merely its name.
    #[test]
    fn a_signature_does_not_verify_after_its_operation_fields_change() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let signed = ClaimOperation::Delegate {
            branch: "feat/x",
            lease_seconds: 60,
            paths: &["src/lib.rs".to_string()],
            symbols: &[],
        };
        let authentication = signed_authentication(&signing_key, "node-1", &signed);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));

        let lib_rs = vec!["src/lib.rs".to_string()];
        let other_rs = vec!["src/other.rs".to_string()];
        let cases: Vec<ClaimOperation> = vec![
            ClaimOperation::Delegate {
                branch: "feat/y",
                lease_seconds: 60,
                paths: &lib_rs,
                symbols: &[],
            },
            ClaimOperation::Delegate {
                branch: "feat/x",
                lease_seconds: 120,
                paths: &lib_rs,
                symbols: &[],
            },
            ClaimOperation::Delegate {
                branch: "feat/x",
                lease_seconds: 60,
                paths: &other_rs,
                symbols: &[],
            },
        ];
        for changed in cases {
            assert_eq!(
                verify(
                    "tenant-a",
                    "repo-a",
                    "task-1",
                    "owner-1",
                    &changed,
                    Some(&authentication),
                    &resolution,
                    fixed_now(),
                ),
                Err(ClaimAuthRefusal::BadSignature)
            );
        }
    }

    /// `Recover` carries `expected_owner` and `reason` beyond what any other
    /// operation signs; both are bound too.
    #[test]
    fn a_recover_signature_does_not_verify_after_expected_owner_or_reason_change() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let signed = ClaimOperation::Recover {
            expected_owner: "stranded-agent",
            branch: "feat/x",
            lease_seconds: 300,
            paths: &[],
            symbols: &[],
            reason: "lease expired",
        };
        let authentication = signed_authentication(&signing_key, "node-1", &signed);
        let resolution =
            KeyResolution::Resolved(record(signing_key.verifying_key().to_bytes().to_vec()));

        let different_owner = ClaimOperation::Recover {
            expected_owner: "a-different-agent",
            branch: "feat/x",
            lease_seconds: 300,
            paths: &[],
            symbols: &[],
            reason: "lease expired",
        };
        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-1",
                "owner-1",
                &different_owner,
                Some(&authentication),
                &resolution,
                fixed_now(),
            ),
            Err(ClaimAuthRefusal::BadSignature)
        );

        let different_reason = ClaimOperation::Recover {
            expected_owner: "stranded-agent",
            branch: "feat/x",
            lease_seconds: 300,
            paths: &[],
            symbols: &[],
            reason: "a made-up reason",
        };
        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                "task-1",
                "owner-1",
                &different_reason,
                Some(&authentication),
                &resolution,
                fixed_now(),
            ),
            Err(ClaimAuthRefusal::BadSignature)
        );
    }
}
