use ackplane_client::{
    authenticate,
    companion::wire::{Claim, NodeReply},
    ClaimClient, ClaimOperation, ClientError,
};
use ackplane_protocol::{
    enrollment_status_auth::{enrollment_status_signing_bytes, EnrollmentStatusOperation},
    v1,
};
use prost::Message;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use super::{NodeService, ServiceSigner};

impl NodeService {
    pub(super) async fn status(&self) -> Result<v1::EnrollmentStatusResult, ClientError> {
        let mut nonce = vec![0; 16];
        getrandom::getrandom(&mut nonce).map_err(|_| ackplane_client::SigningError::Unavailable)?;
        let mut authentication = v1::EnrollmentStatusAuthentication {
            node_id: self.binding.node_id.clone(),
            key_fingerprint: self.identity.fingerprint.clone(),
            signed_at: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .map_err(|_| ackplane_client::SigningError::Refused)?,
            nonce,
            signature: Vec::new(),
        };
        let bytes = enrollment_status_signing_bytes(
            &self.binding.tenant_id,
            &self.binding.repository_id,
            EnrollmentStatusOperation::Check,
            &authentication,
        );
        authentication.signature = self
            .signer
            .sign("enrollment.status", &self.binding, &bytes)
            .map_err(|_| ackplane_client::SigningError::Unavailable)?
            .as_bytes()
            .to_vec();
        ackplane_client::EnrollmentClient::connect(&self.endpoint)
            .await?
            .check_enrollment_status(v1::EnrollmentStatusRequest {
                tenant_id: self.binding.tenant_id.clone(),
                repository_id: self.binding.repository_id.clone(),
                candidate_node_id: self.binding.node_id.clone(),
                candidate_key_fingerprint: self.identity.fingerprint.clone(),
                authentication: Some(authentication),
            })
            .await
    }

    pub(super) async fn claim(&self, request: Claim) -> Result<NodeReply, ClientError> {
        let signer = ServiceSigner(self);
        let tenant = &self.binding.tenant_id;
        let repository = &self.binding.repository_id;
        let bytes = match request {
            Claim::Delegate {
                task_id,
                owner_id,
                branch,
                lease_seconds,
                paths,
                symbols,
            } => {
                let authentication = authenticate(
                    &signer,
                    tenant,
                    repository,
                    &task_id,
                    &owner_id,
                    &ClaimOperation::Delegate {
                        branch: &branch,
                        lease_seconds,
                        paths: &paths,
                        symbols: &symbols,
                    },
                )?;
                ClaimClient::connect(&self.endpoint)
                    .await?
                    .delegate_claim(v1::ClaimLeaseRequest {
                        tenant_id: tenant.clone(),
                        repository_id: repository.clone(),
                        task_id,
                        owner_id,
                        branch,
                        lease_seconds,
                        paths,
                        symbols,
                        authentication: Some(authentication),
                    })
                    .await?
                    .encode_to_vec()
            }
            Claim::Renew {
                task_id,
                owner_id,
                lease_seconds,
            } => {
                let authentication = authenticate(
                    &signer,
                    tenant,
                    repository,
                    &task_id,
                    &owner_id,
                    &ClaimOperation::Renew { lease_seconds },
                )?;
                ClaimClient::connect(&self.endpoint)
                    .await?
                    .renew_claim(v1::ClaimRenewRequest {
                        tenant_id: tenant.clone(),
                        repository_id: repository.clone(),
                        task_id,
                        owner_id,
                        lease_seconds,
                        authentication: Some(authentication),
                    })
                    .await?
                    .encode_to_vec()
            }
            Claim::Release { task_id, owner_id } => {
                let authentication = authenticate(
                    &signer,
                    tenant,
                    repository,
                    &task_id,
                    &owner_id,
                    &ClaimOperation::Release,
                )?;
                ClaimClient::connect(&self.endpoint)
                    .await?
                    .release_claim(v1::ClaimReleaseRequest {
                        tenant_id: tenant.clone(),
                        repository_id: repository.clone(),
                        task_id,
                        owner_id,
                        authentication: Some(authentication),
                    })
                    .await?
                    .encode_to_vec()
            }
            Claim::Recover {
                task_id,
                owner_id,
                expected_owner,
                branch,
                lease_seconds,
                paths,
                symbols,
                reason,
            } => {
                let authentication = authenticate(
                    &signer,
                    tenant,
                    repository,
                    &task_id,
                    &owner_id,
                    &ClaimOperation::Recover {
                        expected_owner: &expected_owner,
                        branch: &branch,
                        lease_seconds,
                        paths: &paths,
                        symbols: &symbols,
                        reason: &reason,
                    },
                )?;
                ClaimClient::connect(&self.endpoint)
                    .await?
                    .recover_claim(v1::ClaimRecoverRequest {
                        tenant_id: tenant.clone(),
                        repository_id: repository.clone(),
                        task_id,
                        owner_id,
                        expected_owner,
                        branch,
                        lease_seconds,
                        paths,
                        symbols,
                        reason,
                        authentication: Some(authentication),
                    })
                    .await?
                    .encode_to_vec()
            }
            Claim::Park { task_id, owner_id } => {
                let authentication = authenticate(
                    &signer,
                    tenant,
                    repository,
                    &task_id,
                    &owner_id,
                    &ClaimOperation::Park,
                )?;
                ClaimClient::connect(&self.endpoint)
                    .await?
                    .park_claim(v1::ClaimParkRequest {
                        tenant_id: tenant.clone(),
                        repository_id: repository.clone(),
                        task_id,
                        owner_id,
                        authentication: Some(authentication),
                    })
                    .await?
                    .encode_to_vec()
            }
            Claim::Answer {
                task_id,
                owner_id,
                lease_seconds,
            } => {
                let authentication = authenticate(
                    &signer,
                    tenant,
                    repository,
                    &task_id,
                    &owner_id,
                    &ClaimOperation::Answer { lease_seconds },
                )?;
                ClaimClient::connect(&self.endpoint)
                    .await?
                    .answer_claim(v1::ClaimAnswerRequest {
                        tenant_id: tenant.clone(),
                        repository_id: repository.clone(),
                        task_id,
                        owner_id,
                        lease_seconds,
                        authentication: Some(authentication),
                    })
                    .await?
                    .encode_to_vec()
            }
        };
        Ok(NodeReply::Payload(bytes))
    }
}
