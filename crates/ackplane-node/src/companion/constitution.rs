use ackplane_client::{
    companion::wire::NodeReply, connect_channel, ClaimSigner, ClientError, SigningError,
};
use ackplane_protocol::{
    constitution_auth::{constitution_signing_bytes, ConstitutionOperation},
    v1::{self, constitution_service_client::ConstitutionServiceClient},
};
use prost::Message;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use super::{NodeService, ServiceSigner};

impl NodeService {
    fn constitution_auth(
        &self,
        operation: ConstitutionOperation<'_>,
    ) -> Result<v1::ConstitutionAuthentication, ClientError> {
        let mut nonce = vec![0; 16];
        getrandom::getrandom(&mut nonce).map_err(|_| SigningError::Unavailable)?;
        let mut authentication = v1::ConstitutionAuthentication {
            node_id: self.binding.node_id.clone(),
            signing_key_id: self.binding.key_id.clone(),
            signed_at: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .map_err(|_| SigningError::Refused)?,
            nonce,
            signature: Vec::new(),
        };
        authentication.signature = ServiceSigner(self).sign(&constitution_signing_bytes(
            &self.binding.tenant_id,
            &self.binding.repository_id,
            &operation,
            &authentication,
        ))?;
        Ok(authentication)
    }

    pub(super) async fn active_constitution(&self) -> Result<NodeReply, ClientError> {
        let request = v1::GetActiveConstitutionRequest {
            tenant_id: self.binding.tenant_id.clone(),
            repository_id: self.binding.repository_id.clone(),
            authentication: Some(self.constitution_auth(ConstitutionOperation::GetActive)?),
        };
        let result = ConstitutionServiceClient::new(connect_channel(&self.endpoint).await?)
            .get_active_constitution(request)
            .await?
            .into_inner();
        Ok(NodeReply::Payload(result.encode_to_vec()))
    }

    pub(super) async fn publish_constitution(
        &self,
        bytes: &[u8],
    ) -> Result<NodeReply, ClientError> {
        let mut snapshot = v1::PublishConstitutionSnapshotRequest::decode(bytes)
            .map_err(|_| SigningError::Refused)?;
        if snapshot.tenant_id != self.binding.tenant_id
            || snapshot.repository_id != self.binding.repository_id
            || snapshot.authentication.is_some()
        {
            return Err(SigningError::IdentityMismatch.into());
        }
        snapshot.authentication = Some(self.constitution_auth(ConstitutionOperation::Publish {
            version_id: &snapshot.version_id,
            version: snapshot.version,
            status: &snapshot.status,
            clause_count: snapshot.clauses.len() as u32,
        })?);
        let result = ConstitutionServiceClient::new(connect_channel(&self.endpoint).await?)
            .publish_constitution_snapshot(snapshot)
            .await?
            .into_inner();
        Ok(NodeReply::Payload(result.encode_to_vec()))
    }
}
