//! Verifying that an envelope was signed by the key it claims.
//!
//! Split from the ledger deliberately: `sync::translate` decides whether the
//! BYTES are well formed, and this decides whether the SENDER is who the
//! envelope says. Both refuse before anything is appended, because a forged
//! record that reaches storage has already been trusted.

use ackplane_protocol::v1;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};

use crate::ledger::ProvenanceClass;
use crate::signing_keys::KeyResolution;

/// Domain separation for envelope signatures.
///
/// Distinct from the enrolment activation domain so a signature captured from
/// one ceremony can never be replayed as the other.
const ENVELOPE_DOMAIN: &[u8] = b"mindleak.ackplane.v1.envelope\0";

/// The exact bytes a node signs, per ADR-0084 decision 4.
///
/// Built from the WIRE envelope rather than the translated one, and that is not
/// incidental: `occurred_at` arrives as an RFC 3339 string and the domain type
/// holds a `SystemTime`. Re-formatting it would produce different bytes than
/// the node signed — the same instant, a different string — and every signature
/// would fail for a reason that looks like forgery.
///
/// Every field is length-delimited, following `enrollment::activation_challenge_bytes`,
/// so adjacent fields can never be reinterpreted as a different tuple.
pub fn envelope_signing_bytes(wire: &v1::EventEnvelope) -> Vec<u8> {
    let sequence = wire.producer_sequence.to_be_bytes();
    let fields: [&[u8]; 10] = [
        wire.schema_version.as_bytes(),
        wire.tenant_id.as_bytes(),
        wire.repository_id.as_bytes(),
        wire.producer_id.as_bytes(),
        &sequence,
        wire.occurred_at.as_bytes(),
        wire.payload_type.as_bytes(),
        &wire.payload_digest,
        &wire.previous_envelope_digest,
        wire.signing_key_id.as_bytes(),
    ];

    let mut bytes = Vec::with_capacity(
        ENVELOPE_DOMAIN.len() + fields.iter().map(|field| 4 + field.len()).sum::<usize>(),
    );
    bytes.extend_from_slice(ENVELOPE_DOMAIN);
    for field in fields {
        bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
        bytes.extend_from_slice(field);
    }
    bytes
}

/// Why an envelope was refused at the trust boundary.
///
/// Each variant is a distinct security story. Collapsing them would lose the
/// one an auditor needs: "we hold no such key" and "that key is not yours" are
/// different incidents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureRefusal {
    /// The declared class needs a signature and the envelope carried none.
    Unsigned,
    /// The declared class needs a key id and the envelope named none.
    Unidentified,
    /// The named key is not one this authority holds.
    UnknownKey,
    /// The key exists but is bound to a different tenant, repository or node.
    BindingMismatch,
    /// The key's authority had not begun, or had ended, when this arrived.
    KeyNotInForce,
    /// The bytes do not verify under the resolved key.
    BadSignature,
    /// The declared class is one this authority cannot substantiate.
    Unsubstantiated,
}

impl SignatureRefusal {
    /// The wire reason. `Unauthorized` only where a real key was presented for
    /// an identity it does not cover — everything else is a failure to
    /// authenticate at all.
    pub fn reason(self) -> v1::RejectionReason {
        match self {
            Self::BindingMismatch => v1::RejectionReason::Unauthorized,
            _ => v1::RejectionReason::Unauthenticated,
        }
    }

    pub fn diagnostic(self) -> &'static str {
        match self {
            Self::Unsigned => {
                "the declared provenance class requires a signature and none was sent"
            }
            Self::Unidentified => {
                "the declared provenance class requires signing_key_id and none was sent"
            }
            Self::UnknownKey => "signing_key_id names no key this authority holds",
            Self::BindingMismatch => {
                "that signing key is enrolled to a different tenant, repository or node"
            }
            Self::KeyNotInForce => {
                "the signing key was not in force when this record arrived: it is revoked, \
                 expired, retired, or not yet activated"
            }
            Self::BadSignature => "the signature does not verify under the enrolled key",
            Self::Unsubstantiated => {
                "this authority cannot substantiate the declared provenance class; \
                 declare enrolled_node and sign, or unverified_attribution"
            }
        }
    }
}

/// Decide whether an envelope may be appended, given the key its id resolved to.
///
/// Pure: no database, no container, no network (ADR-0088 clause 2), the same
/// split `sync::translate` and `signing_keys::judge` use. The lookup is the
/// easy half; this is the half that decides whether evidence is trustworthy.
///
/// DECLARED TRUST IS REFUSED, NOT DOWNGRADED. A sender may name any class, and
/// this authority can substantiate exactly one of them — `EnrolledNode`, by
/// checking a signature against an enrolled key. OIDC and provider attestation
/// are not implemented, so `AuthenticatedPrincipal` and `ProviderAttested` are
/// refused rather than quietly stored as something weaker. Downgrading would
/// write a class the producer never claimed and tell it nothing, so it would go
/// on believing its evidence carried a trust it does not; a non-retryable
/// refusal naming the class is the only outcome that reaches the sender.
pub fn verify(
    wire: &v1::EventEnvelope,
    provenance: ProvenanceClass,
    resolution: &KeyResolution,
) -> Result<(), SignatureRefusal> {
    match provenance {
        // Claims nothing, so it needs no key. A signature it did send is still
        // checked: accepting bytes that fail their own signature would store a
        // record nobody can later explain.
        ProvenanceClass::UnverifiedAttribution => {
            if wire.signature.is_empty() {
                return Ok(());
            }
            check_signature(wire, resolution)
        }
        ProvenanceClass::EnrolledNode => {
            if wire.signing_key_id.is_empty() {
                return Err(SignatureRefusal::Unidentified);
            }
            if wire.signature.is_empty() {
                return Err(SignatureRefusal::Unsigned);
            }
            check_signature(wire, resolution)
        }
        ProvenanceClass::AuthenticatedPrincipal | ProvenanceClass::ProviderAttested => {
            Err(SignatureRefusal::Unsubstantiated)
        }
    }
}

fn check_signature(
    wire: &v1::EventEnvelope,
    resolution: &KeyResolution,
) -> Result<(), SignatureRefusal> {
    let record = match resolution {
        KeyResolution::Resolved(record) => record,
        KeyResolution::Unknown => return Err(SignatureRefusal::UnknownKey),
        KeyResolution::BindingMismatch => return Err(SignatureRefusal::BindingMismatch),
        KeyResolution::NotYetActive
        | KeyResolution::Expired
        | KeyResolution::Revoked
        | KeyResolution::Retired => return Err(SignatureRefusal::KeyNotInForce),
    };

    let key = <&[u8; 32]>::try_from(record.public_key.as_slice())
        .ok()
        .and_then(|bytes| VerifyingKey::from_bytes(bytes).ok())
        .ok_or(SignatureRefusal::BadSignature)?;
    let signature =
        Signature::from_slice(&wire.signature).map_err(|_| SignatureRefusal::BadSignature)?;

    key.verify(&envelope_signing_bytes(wire), &signature)
        .map_err(|_| SignatureRefusal::BadSignature)
}
