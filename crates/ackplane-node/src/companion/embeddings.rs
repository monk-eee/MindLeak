use ackplane_client::{
    companion::wire::NodeReply, connect_channel, ClaimSigner, ClientError, SigningError,
};
use ackplane_protocol::{
    projection_embedding_auth::{projection_embedding_signing_bytes, ProjectionEmbeddingOperation},
    v1::{self, projection_embedding_service_client::ProjectionEmbeddingServiceClient},
};
use prost::Message;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use super::{NodeService, ServiceSigner};

impl NodeService {
    fn projection_embedding_auth(
        &self,
        operation: ProjectionEmbeddingOperation<'_>,
    ) -> Result<v1::ProjectionEmbeddingAuthentication, ClientError> {
        operation.validate().map_err(|_| SigningError::Refused)?;
        let mut nonce = vec![0; 16];
        getrandom::getrandom(&mut nonce).map_err(|_| SigningError::Unavailable)?;
        let mut authentication = v1::ProjectionEmbeddingAuthentication {
            signing_key_id: self.binding.key_id.clone(),
            node_id: self.binding.node_id.clone(),
            signed_at: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .map_err(|_| SigningError::Refused)?,
            nonce,
            signature: Vec::new(),
        };
        authentication.signature =
            ServiceSigner(self).sign(&projection_embedding_signing_bytes(
                &self.binding.tenant_id,
                &self.binding.repository_id,
                &operation,
                &authentication,
            ))?;
        Ok(authentication)
    }

    fn missing_projection_embeddings_request(
        &self,
        model: String,
        limit: u32,
    ) -> Result<v1::ListMissingProjectionEmbeddingsRequest, ClientError> {
        let authentication =
            self.projection_embedding_auth(ProjectionEmbeddingOperation::ListMissing {
                model: &model,
                limit,
            })?;
        Ok(v1::ListMissingProjectionEmbeddingsRequest {
            tenant_id: self.binding.tenant_id.clone(),
            repository_id: self.binding.repository_id.clone(),
            model,
            limit,
            authentication: Some(authentication),
        })
    }

    fn publish_projection_embedding_request(
        &self,
        bytes: &[u8],
        model: String,
        embedding: Vec<f32>,
    ) -> Result<v1::PublishProjectionEmbeddingRequest, ClientError> {
        let source =
            v1::ProjectionEmbeddingSource::decode(bytes).map_err(|_| SigningError::Refused)?;
        if source.encode_to_vec() != bytes {
            return Err(SigningError::Refused.into());
        }
        let authentication =
            self.projection_embedding_auth(ProjectionEmbeddingOperation::Publish {
                source: &source,
                model: &model,
                embedding: &embedding,
            })?;
        Ok(v1::PublishProjectionEmbeddingRequest {
            tenant_id: self.binding.tenant_id.clone(),
            repository_id: self.binding.repository_id.clone(),
            source: Some(source),
            model,
            embedding,
            authentication: Some(authentication),
        })
    }

    pub(super) async fn missing_projection_embeddings(
        &self,
        model: String,
        limit: u32,
    ) -> Result<NodeReply, ClientError> {
        let request = self.missing_projection_embeddings_request(model, limit)?;
        let result = ProjectionEmbeddingServiceClient::new(connect_channel(&self.endpoint).await?)
            .list_missing_projection_embeddings(request)
            .await?
            .into_inner();
        Ok(NodeReply::Payload(result.encode_to_vec()))
    }

    pub(super) async fn publish_projection_embedding(
        &self,
        source: &[u8],
        model: String,
        embedding: Vec<f32>,
    ) -> Result<NodeReply, ClientError> {
        let request = self.publish_projection_embedding_request(source, model, embedding)?;
        let result = ProjectionEmbeddingServiceClient::new(connect_channel(&self.endpoint).await?)
            .publish_projection_embedding(request)
            .await?
            .into_inner();
        Ok(NodeReply::Payload(result.encode_to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, sync::Arc};

    use ackplane_client::companion::wire::Operation;
    use ed25519_dalek::{Signature, VerifyingKey};

    use super::*;
    use crate::SoftwareProvider;

    fn service() -> NodeService {
        NodeService::new(
            "tenant".into(),
            "repository".into(),
            "http://127.0.0.1:1".into(),
            Arc::new(SoftwareProvider::generate("tenant", "repository", "node")),
        )
        .unwrap()
    }

    fn source() -> v1::ProjectionEmbeddingSource {
        v1::ProjectionEmbeddingSource {
            node_id: "artifact:src/lib.rs".into(),
            label: "source label".into(),
        }
    }

    #[test]
    fn requests_sign_bound_identity_and_payload_with_fresh_nonces() {
        let service = service();
        let source = source();
        let started = OffsetDateTime::now_utc();
        let mut nonces = HashSet::new();
        let public_key = VerifyingKey::from_bytes(&service.signer.identity().public_key).unwrap();
        for _ in 0..2 {
            let missing = service
                .missing_projection_embeddings_request("model".into(), 0)
                .unwrap();
            let published = service
                .publish_projection_embedding_request(
                    &source.encode_to_vec(),
                    "model".into(),
                    vec![0.25; 768],
                )
                .unwrap();
            assert_eq!(missing.model, "model");
            assert_eq!(missing.limit, 0);
            assert_eq!(published.source.as_ref(), Some(&source));
            assert_eq!(published.model, "model");
            assert_eq!(published.embedding, vec![0.25; 768]);
            for (tenant, repository, operation, authentication) in [
                (
                    &missing.tenant_id,
                    &missing.repository_id,
                    ProjectionEmbeddingOperation::ListMissing {
                        model: &missing.model,
                        limit: missing.limit,
                    },
                    missing.authentication.as_ref().unwrap(),
                ),
                (
                    &published.tenant_id,
                    &published.repository_id,
                    ProjectionEmbeddingOperation::Publish {
                        source: published.source.as_ref().unwrap(),
                        model: &published.model,
                        embedding: &published.embedding,
                    },
                    published.authentication.as_ref().unwrap(),
                ),
            ] {
                assert_eq!(tenant, "tenant");
                assert_eq!(repository, "repository");
                assert_eq!(authentication.node_id, "node");
                assert_eq!(authentication.signing_key_id, service.binding.key_id);
                assert_eq!(authentication.nonce.len(), 16);
                assert!(nonces.insert(authentication.nonce.clone()));
                let signed_at = OffsetDateTime::parse(&authentication.signed_at, &Rfc3339).unwrap();
                assert!(signed_at >= started && signed_at <= OffsetDateTime::now_utc());
                let bytes = projection_embedding_signing_bytes(
                    tenant,
                    repository,
                    &operation,
                    authentication,
                );
                let signature = Signature::from_slice(&authentication.signature).unwrap();
                public_key.verify_strict(&bytes, &signature).unwrap();
                let wrong_scope = projection_embedding_signing_bytes(
                    "other",
                    repository,
                    &operation,
                    authentication,
                );
                assert!(public_key.verify_strict(&wrong_scope, &signature).is_err());
            }
        }
        assert_eq!(nonces.len(), 4);
    }

    #[test]
    fn list_limits_preserve_zero_default_and_refuse_more_than_100() {
        let service = service();
        for limit in [0, 20, 100] {
            assert_eq!(
                service
                    .missing_projection_embeddings_request("model".into(), limit)
                    .unwrap()
                    .limit,
                limit
            );
        }
        for limit in [101, u32::MAX] {
            assert!(matches!(
                service.missing_projection_embeddings_request("model".into(), limit),
                Err(ClientError::Signing(SigningError::Refused))
            ));
        }
    }

    #[test]
    fn models_are_nonblank_and_bounded_by_bytes_for_both_operations() {
        let service = service();
        let source = source().encode_to_vec();
        for model in [
            String::new(),
            " \t\r\n".into(),
            "m".repeat(129),
            "\u{00e9}".repeat(65),
        ] {
            assert!(matches!(
                service.missing_projection_embeddings_request(model.clone(), 20),
                Err(ClientError::Signing(SigningError::Refused))
            ));
            assert!(matches!(
                service.publish_projection_embedding_request(&source, model, vec![1.0; 768]),
                Err(ClientError::Signing(SigningError::Refused))
            ));
        }
        for model in ["m".into(), "m".repeat(128), "\u{00e9}".repeat(64)] {
            service
                .missing_projection_embeddings_request(model.clone(), 20)
                .unwrap();
            service
                .publish_projection_embedding_request(&source, model, vec![1.0; 768])
                .unwrap();
        }
    }

    #[test]
    fn source_bounds_allow_an_empty_label_but_not_an_empty_node_id() {
        let service = service();
        for (node_id, label) in [
            (String::new(), String::new()),
            (" \t".into(), String::new()),
            ("n".repeat(2049), String::new()),
            ("node".into(), "l".repeat(4097)),
        ] {
            let source = v1::ProjectionEmbeddingSource { node_id, label }.encode_to_vec();
            assert!(matches!(
                service.publish_projection_embedding_request(
                    &source,
                    "model".into(),
                    vec![1.0; 768]
                ),
                Err(ClientError::Signing(SigningError::Refused))
            ));
        }
        for (node_id, label) in [
            ("n".into(), String::new()),
            ("n".repeat(2048), "l".repeat(4096)),
        ] {
            let source = v1::ProjectionEmbeddingSource { node_id, label };
            let request = service
                .publish_projection_embedding_request(
                    &source.encode_to_vec(),
                    "model".into(),
                    vec![1.0; 768],
                )
                .unwrap();
            assert_eq!(request.source, Some(source));
        }
    }

    #[test]
    fn vectors_require_768_finite_components_and_at_least_one_nonzero() {
        let service = service();
        let source = source().encode_to_vec();
        for embedding in [
            vec![],
            vec![1.0; 767],
            vec![1.0; 769],
            vec![0.0; 768],
            vec![-0.0; 768],
        ] {
            assert!(matches!(
                service.publish_projection_embedding_request(&source, "model".into(), embedding),
                Err(ClientError::Signing(SigningError::Refused))
            ));
        }
        for component in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut embedding = vec![1.0; 768];
            embedding[767] = component;
            assert!(matches!(
                service.publish_projection_embedding_request(&source, "model".into(), embedding),
                Err(ClientError::Signing(SigningError::Refused))
            ));
        }
        let mut embedding = vec![0.0; 768];
        embedding[767] = f32::from_bits(1);
        service
            .publish_projection_embedding_request(&source, "model".into(), embedding)
            .unwrap();
    }

    #[test]
    fn source_decode_refuses_malformed_noncanonical_and_full_request_bytes() {
        let service = service();
        let source = source().encode_to_vec();
        let request = service
            .publish_projection_embedding_request(&source, "model".into(), vec![1.0; 768])
            .unwrap();
        for bytes in [
            vec![0xff],
            [source.as_slice(), &[0x18, 1]].concat(),
            [source.as_slice(), source.as_slice()].concat(),
            request.encode_to_vec(),
        ] {
            assert!(matches!(
                service.publish_projection_embedding_request(
                    &bytes,
                    "model".into(),
                    vec![1.0; 768]
                ),
                Err(ClientError::Signing(SigningError::Refused))
            ));
        }
    }

    #[test]
    fn provider_binding_refusal_is_not_bypassed_for_either_operation() {
        let service = NodeService::new(
            "other-tenant".into(),
            "repository".into(),
            "http://127.0.0.1:1".into(),
            Arc::new(SoftwareProvider::generate("tenant", "repository", "node")),
        )
        .unwrap();
        assert!(matches!(
            service.missing_projection_embeddings_request("model".into(), 20),
            Err(ClientError::Signing(SigningError::Unavailable))
        ));
        assert!(matches!(
            service.publish_projection_embedding_request(
                &source().encode_to_vec(),
                "model".into(),
                vec![1.0; 768]
            ),
            Err(ClientError::Signing(SigningError::Unavailable))
        ));
    }

    #[tokio::test]
    async fn dispatch_refuses_invalid_embedding_payloads_before_connecting() {
        let service = service();
        for operation in [
            Operation::ProjectionEmbeddingsMissing {
                model: "model".into(),
                limit: 101,
            },
            Operation::ProjectionEmbeddingPublish {
                source: source().encode_to_vec(),
                model: "model".into(),
                embedding: vec![0.0; 768],
            },
        ] {
            assert!(matches!(
                service.dispatch(operation).await,
                Err(ClientError::Signing(SigningError::Refused))
            ));
        }
    }
}
