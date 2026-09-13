//! Enrolled-node projection embedding signatures (ADR-0148).

use crate::{signing_bytes::push_field, v1};

pub const PROJECTION_EMBEDDING_DOMAIN: &[u8] = b"mindleak.ackplane.v1.projection-embedding\0";
pub const DEFAULT_EMBEDDING_LIST_LIMIT: u32 = 20;
pub const MAX_EMBEDDING_LIST_LIMIT: u32 = 100;

impl v1::ProjectionEmbeddingSource {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.node_id.trim().is_empty() || self.node_id.len() > 2048 {
            return Err("source node_id must contain 1 to 2048 bytes");
        }
        if self.label.len() > 4096 {
            return Err("source label must not exceed 4096 bytes");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ProjectionEmbeddingOperation<'a> {
    ListMissing {
        model: &'a str,
        limit: u32,
    },
    Publish {
        source: &'a v1::ProjectionEmbeddingSource,
        model: &'a str,
        embedding: &'a [f32],
    },
}

impl ProjectionEmbeddingOperation<'_> {
    pub fn validate(&self) -> Result<(), &'static str> {
        let model = match self {
            Self::ListMissing { model, limit } => {
                if *limit > MAX_EMBEDDING_LIST_LIMIT {
                    return Err("limit must not exceed 100");
                }
                model
            }
            Self::Publish {
                source,
                model,
                embedding,
            } => {
                source.validate()?;
                if embedding.len() != 768
                    || embedding.iter().any(|component| !component.is_finite())
                    || !embedding.iter().any(|component| *component != 0.0)
                {
                    return Err("embedding must contain 768 finite components and be nonzero");
                }
                model
            }
        };
        if model.trim().is_empty() || model.len() > 128 {
            return Err("model must contain 1 to 128 bytes");
        }
        Ok(())
    }
}

/// Every field, including the source snapshot and vector bits, is length-delimited.
pub fn projection_embedding_signing_bytes(
    tenant_id: &str,
    repository_id: &str,
    operation: &ProjectionEmbeddingOperation<'_>,
    authentication: &v1::ProjectionEmbeddingAuthentication,
) -> Vec<u8> {
    let identity_fields: [&[u8]; 6] = [
        authentication.signing_key_id.as_bytes(),
        authentication.node_id.as_bytes(),
        authentication.signed_at.as_bytes(),
        &authentication.nonce,
        tenant_id.as_bytes(),
        repository_id.as_bytes(),
    ];
    let mut bytes = PROJECTION_EMBEDDING_DOMAIN.to_vec();
    for field in identity_fields {
        push_field(&mut bytes, field);
    }
    match operation {
        ProjectionEmbeddingOperation::ListMissing { model, limit } => {
            push_field(&mut bytes, b"list_missing");
            push_field(&mut bytes, model.as_bytes());
            push_field(&mut bytes, &limit.to_be_bytes());
        }
        ProjectionEmbeddingOperation::Publish {
            source,
            model,
            embedding,
        } => {
            push_field(&mut bytes, b"publish");
            push_field(&mut bytes, source.node_id.as_bytes());
            push_field(&mut bytes, source.label.as_bytes());
            push_field(&mut bytes, model.as_bytes());
            push_field(&mut bytes, &(embedding.len() as u64).to_be_bytes());
            for component in *embedding {
                push_field(&mut bytes, &component.to_bits().to_be_bytes());
            }
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::*;

    fn authentication() -> v1::ProjectionEmbeddingAuthentication {
        v1::ProjectionEmbeddingAuthentication {
            signing_key_id: "key-1".to_string(),
            node_id: "node-1".to_string(),
            signed_at: "2026-09-14T00:00:00Z".to_string(),
            nonce: vec![1; 16],
            signature: Vec::new(),
        }
    }

    #[test]
    fn identical_fields_have_stable_bytes_independent_of_the_signature() {
        let operation = ProjectionEmbeddingOperation::ListMissing {
            model: "model-a",
            limit: 20,
        };
        let mut authentication = authentication();
        let original =
            projection_embedding_signing_bytes("tenant-a", "repo-a", &operation, &authentication);
        authentication.signature = vec![8; 64];
        assert_eq!(
            original,
            projection_embedding_signing_bytes("tenant-a", "repo-a", &operation, &authentication)
        );
        assert!(original.starts_with(PROJECTION_EMBEDDING_DOMAIN));
        assert!(!original.starts_with(crate::knowledge_auth::KNOWLEDGE_DOMAIN));
        assert!(!original.starts_with(crate::claim_auth::CLAIM_DOMAIN));
    }

    #[test]
    fn every_identity_field_changes_the_signed_bytes() {
        let operation = ProjectionEmbeddingOperation::ListMissing {
            model: "model-a",
            limit: 20,
        };
        let authentication = authentication();
        let original =
            projection_embedding_signing_bytes("tenant-a", "repo-a", &operation, &authentication);
        for (tenant, repository) in [("tenant-b", "repo-a"), ("tenant-a", "repo-b")] {
            assert_ne!(
                original,
                projection_embedding_signing_bytes(tenant, repository, &operation, &authentication)
            );
        }
        for field in 0..4 {
            let mut changed = authentication.clone();
            match field {
                0 => changed.signing_key_id.push('x'),
                1 => changed.node_id.push('x'),
                2 => changed.signed_at.push('x'),
                3 => changed.nonce.push(2),
                _ => unreachable!(),
            }
            assert_ne!(
                original,
                projection_embedding_signing_bytes("tenant-a", "repo-a", &operation, &changed)
            );
        }
    }

    #[test]
    fn list_model_and_limit_are_signed() {
        let authentication = authentication();
        let original = projection_embedding_signing_bytes(
            "tenant-a",
            "repo-a",
            &ProjectionEmbeddingOperation::ListMissing {
                model: "model-a",
                limit: 20,
            },
            &authentication,
        );
        for (model, limit) in [("model-b", 20), ("model-a", 21)] {
            assert_ne!(
                original,
                projection_embedding_signing_bytes(
                    "tenant-a",
                    "repo-a",
                    &ProjectionEmbeddingOperation::ListMissing { model, limit },
                    &authentication,
                )
            );
        }
    }

    #[test]
    fn publication_signs_source_model_vector_length_and_each_component() {
        let source = v1::ProjectionEmbeddingSource {
            node_id: "artifact:src/lib.rs".to_string(),
            label: "source label".to_string(),
        };
        let authentication = authentication();
        let original = projection_embedding_signing_bytes(
            "tenant-a",
            "repo-a",
            &ProjectionEmbeddingOperation::Publish {
                source: &source,
                model: "model-a",
                embedding: &[0.0, 1.0],
            },
            &authentication,
        );
        for field in 0..6 {
            let mut changed_source = source.clone();
            let mut model = "model-a";
            let mut vector = vec![0.0, 1.0];
            match field {
                0 => changed_source.node_id.push('x'),
                1 => changed_source.label.push('x'),
                2 => model = "model-b",
                3 => vector.push(2.0),
                4 => vector[0] = -0.0,
                5 => vector[1] = 2.0,
                _ => unreachable!(),
            }
            assert_ne!(
                original,
                projection_embedding_signing_bytes(
                    "tenant-a",
                    "repo-a",
                    &ProjectionEmbeddingOperation::Publish {
                        source: &changed_source,
                        model,
                        embedding: &vector,
                    },
                    &authentication,
                )
            );
        }
    }

    #[test]
    fn operations_cannot_share_a_signature() {
        let source = v1::ProjectionEmbeddingSource::default();
        let authentication = authentication();
        assert_ne!(
            projection_embedding_signing_bytes(
                "tenant-a",
                "repo-a",
                &ProjectionEmbeddingOperation::ListMissing {
                    model: "",
                    limit: 0
                },
                &authentication
            ),
            projection_embedding_signing_bytes(
                "tenant-a",
                "repo-a",
                &ProjectionEmbeddingOperation::Publish {
                    source: &source,
                    model: "",
                    embedding: &[]
                },
                &authentication
            )
        );
    }

    #[test]
    fn publication_round_trips_without_losing_source_or_authentication() {
        let request = v1::PublishProjectionEmbeddingRequest {
            tenant_id: "tenant-a".to_string(),
            repository_id: "repo-a".to_string(),
            source: Some(v1::ProjectionEmbeddingSource {
                node_id: "artifact:src/lib.rs".to_string(),
                label: "source label".to_string(),
            }),
            model: "model-a".to_string(),
            embedding: vec![0.25; 768],
            authentication: Some(authentication()),
        };
        assert_eq!(
            v1::PublishProjectionEmbeddingRequest::decode(request.encode_to_vec().as_slice())
                .unwrap(),
            request
        );
    }
}
