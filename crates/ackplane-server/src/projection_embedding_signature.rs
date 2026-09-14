use std::time::SystemTime;

use ackplane_protocol::projection_embedding_auth::{
    projection_embedding_signing_bytes, ProjectionEmbeddingOperation,
};
use ackplane_protocol::v1::ProjectionEmbeddingAuthentication;
use ed25519_dalek::{Signature, VerifyingKey};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tonic::Status;

use crate::signing_keys::KeyResolution;

pub(crate) fn verify(
    tenant_id: &str,
    repository_id: &str,
    operation: &ProjectionEmbeddingOperation<'_>,
    authentication: &ProjectionEmbeddingAuthentication,
    resolution: &KeyResolution,
    now: SystemTime,
) -> Result<(), Status> {
    if authentication.signing_key_id.trim().is_empty()
        || authentication.node_id.trim().is_empty()
        || authentication.nonce.len() != 16
        || authentication.signature.len() != 64
    {
        return Err(Status::unauthenticated(
            "malformed projection embedding authentication",
        ));
    }
    let signed_at = OffsetDateTime::parse(&authentication.signed_at, &Rfc3339)
        .map_err(|_| Status::unauthenticated("authentication timestamp must be RFC3339"))?;
    if (signed_at - OffsetDateTime::from(now)).abs() > time::Duration::seconds(300) {
        return Err(Status::unauthenticated(
            "authentication timestamp is outside the accepted window",
        ));
    }
    let record = match resolution {
        KeyResolution::Resolved(record) => record,
        KeyResolution::BindingMismatch => {
            return Err(Status::permission_denied(
                "signing key is enrolled to a different scope",
            ));
        }
        KeyResolution::Unknown => return Err(Status::unauthenticated("unknown signing key")),
        KeyResolution::Revoked => {
            return Err(Status::unauthenticated("signing key has been revoked"))
        }
        KeyResolution::NotYetActive | KeyResolution::Expired | KeyResolution::Retired => {
            return Err(Status::unauthenticated(
                "signing key is not currently in force",
            ));
        }
    };
    let key = <&[u8; 32]>::try_from(record.public_key.as_slice())
        .ok()
        .and_then(|bytes| VerifyingKey::from_bytes(bytes).ok())
        .ok_or_else(|| Status::unauthenticated("invalid enrolled verification key"))?;
    let signature = Signature::from_slice(&authentication.signature)
        .map_err(|_| Status::unauthenticated("invalid projection embedding signature"))?;
    let bytes =
        projection_embedding_signing_bytes(tenant_id, repository_id, operation, authentication);
    key.verify_strict(&bytes, &signature)
        .map_err(|_| Status::unauthenticated("projection embedding signature does not verify"))
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;
    use crate::signing_keys::SigningKeyRecord;

    const OPERATION: ProjectionEmbeddingOperation<'static> =
        ProjectionEmbeddingOperation::ListMissing {
            model: "model-a",
            limit: 20,
        };

    fn fixture() -> (ProjectionEmbeddingAuthentication, KeyResolution, SystemTime) {
        let now = SystemTime::now();
        let key = SigningKey::from_bytes(&[17; 32]);
        let mut authentication = ProjectionEmbeddingAuthentication {
            signing_key_id: "key-1".to_string(),
            node_id: "node-1".to_string(),
            signed_at: OffsetDateTime::from(now).format(&Rfc3339).unwrap(),
            nonce: vec![3; 16],
            signature: Vec::new(),
        };
        authentication.signature = key
            .sign(&projection_embedding_signing_bytes(
                "tenant-a",
                "repo-a",
                &OPERATION,
                &authentication,
            ))
            .to_bytes()
            .to_vec();
        let record = SigningKeyRecord {
            signing_key_id: "key-1".to_string(),
            tenant_id: "tenant-a".to_string(),
            repository_id: "repo-a".to_string(),
            node_id: "node-1".to_string(),
            public_key: key.verifying_key().to_bytes().to_vec(),
            public_key_fingerprint: "fixture".to_string(),
            activated_at: SystemTime::UNIX_EPOCH,
            expires_at: None,
        };
        (authentication, KeyResolution::Resolved(record), now)
    }

    #[test]
    fn a_fresh_enrolled_request_verifies() {
        let (authentication, resolution, now) = fixture();
        verify(
            "tenant-a",
            "repo-a",
            &OPERATION,
            &authentication,
            &resolution,
            now,
        )
        .unwrap();
    }

    #[test]
    fn changed_scope_operation_or_signature_is_refused() {
        let (mut authentication, resolution, now) = fixture();
        assert_eq!(
            verify(
                "tenant-b",
                "repo-a",
                &OPERATION,
                &authentication,
                &resolution,
                now
            )
            .unwrap_err()
            .code(),
            tonic::Code::Unauthenticated
        );
        let changed = ProjectionEmbeddingOperation::ListMissing {
            model: "model-b",
            limit: 20,
        };
        assert!(verify(
            "tenant-a",
            "repo-a",
            &changed,
            &authentication,
            &resolution,
            now
        )
        .is_err());
        authentication.signature[0] ^= 1;
        assert!(verify(
            "tenant-a",
            "repo-a",
            &OPERATION,
            &authentication,
            &resolution,
            now
        )
        .is_err());
    }

    #[test]
    fn every_unusable_key_resolution_is_refused() {
        let (authentication, _, now) = fixture();
        for resolution in [
            KeyResolution::Unknown,
            KeyResolution::NotYetActive,
            KeyResolution::Expired,
            KeyResolution::Retired,
            KeyResolution::Revoked,
        ] {
            assert_eq!(
                verify(
                    "tenant-a",
                    "repo-a",
                    &OPERATION,
                    &authentication,
                    &resolution,
                    now
                )
                .unwrap_err()
                .code(),
                tonic::Code::Unauthenticated
            );
        }
        assert_eq!(
            verify(
                "tenant-a",
                "repo-a",
                &OPERATION,
                &authentication,
                &KeyResolution::BindingMismatch,
                now
            )
            .unwrap_err()
            .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn stale_timestamps_and_malformed_authentication_are_refused() {
        let (authentication, resolution, now) = fixture();
        for field in 0..5 {
            let mut changed = authentication.clone();
            match field {
                0 => changed.nonce.clear(),
                1 => changed.signature.clear(),
                2 => changed.signing_key_id.clear(),
                3 => changed.node_id.clear(),
                4 => changed.signed_at = "not-a-timestamp".to_string(),
                _ => unreachable!(),
            }
            assert_eq!(
                verify("tenant-a", "repo-a", &OPERATION, &changed, &resolution, now)
                    .unwrap_err()
                    .code(),
                tonic::Code::Unauthenticated
            );
        }
        for checked_at in [
            now + std::time::Duration::from_secs(301),
            now - std::time::Duration::from_secs(301),
        ] {
            let error = verify(
                "tenant-a",
                "repo-a",
                &OPERATION,
                &authentication,
                &resolution,
                checked_at,
            )
            .unwrap_err();
            assert_eq!(error.code(), tonic::Code::Unauthenticated);
            assert!(error.message().contains("outside the accepted window"));
        }
    }
}
