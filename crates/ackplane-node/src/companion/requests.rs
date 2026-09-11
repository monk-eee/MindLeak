use ackplane_client::{
    companion::wire::{NodeReply, Operation},
    ClientError, SigningError,
};
use ackplane_protocol::v1;
use prost::Message;

use super::NodeService;

impl NodeService {
    pub(super) async fn dispatch(&self, operation: Operation) -> Result<NodeReply, ClientError> {
        match operation {
            Operation::Identity => Ok(NodeReply::Identity(self.identity.clone())),
            Operation::EnrollmentStatus => {
                Ok(NodeReply::Payload(self.status().await?.encode_to_vec()))
            }
            Operation::ConstitutionActive => self.active_constitution().await,
            Operation::ConstitutionPublish { snapshot } => {
                self.publish_constitution(&snapshot).await
            }
            Operation::Claim(claim) => self.claim(claim).await,
            Operation::ActiveClaims => {
                let mut client = ackplane_client::ClaimClient::connect(&self.endpoint).await?;
                Ok(NodeReply::Payload(
                    client
                        .list_active_claims(v1::ActiveClaimsRequest {
                            tenant_id: self.binding.tenant_id.clone(),
                            repository_id: self.binding.repository_id.clone(),
                        })
                        .await?
                        .encode_to_vec(),
                ))
            }
            Operation::WorkList {
                state,
                page,
                page_size,
            } => {
                let mut client = ackplane_client::WorkQueryClient::connect(&self.endpoint).await?;
                Ok(NodeReply::Payload(
                    client
                        .list_work_tasks(v1::ListWorkTasksRequest {
                            tenant_id: self.binding.tenant_id.clone(),
                            repository_id: self.binding.repository_id.clone(),
                            state,
                            page,
                            page_size,
                        })
                        .await?
                        .encode_to_vec(),
                ))
            }
            Operation::WorkDetail { task_id } => {
                let mut client = ackplane_client::WorkQueryClient::connect(&self.endpoint).await?;
                Ok(NodeReply::Payload(
                    client
                        .get_work_task_detail(v1::WorkTaskDetailRequest {
                            tenant_id: self.binding.tenant_id.clone(),
                            repository_id: self.binding.repository_id.clone(),
                            task_id,
                        })
                        .await?
                        .encode_to_vec(),
                ))
            }
            Operation::WorkDoctor => {
                let mut client = ackplane_client::WorkQueryClient::connect(&self.endpoint).await?;
                Ok(NodeReply::Payload(
                    client
                        .get_work_board_doctor(v1::WorkBoardDoctorRequest {
                            tenant_id: self.binding.tenant_id.clone(),
                            repository_id: self.binding.repository_id.clone(),
                        })
                        .await?
                        .encode_to_vec(),
                ))
            }
            Operation::OpenSync { .. } => Err(SigningError::Refused.into()),
        }
    }
}

pub(super) fn refused(error: ClientError) -> NodeReply {
    match error {
        ClientError::ConnectionRefused {
            reason,
            retryable,
            diagnostic,
        } => NodeReply::Refused {
            reason: reason as i32,
            retryable,
            diagnostic,
        },
        ClientError::FrameRefused {
            reason,
            retryable,
            diagnostic,
        } => NodeReply::Frame(
            v1::AckplaneFrame {
                frame: Some(v1::ackplane_frame::Frame::Rejection(v1::Rejection {
                    record_id: String::new(),
                    reason: reason as i32,
                    retryable,
                    diagnostic,
                })),
            }
            .encode_to_vec(),
        ),
        ClientError::Rejected(status) => {
            use tonic::Code;
            let (reason, retryable) = match status.code() {
                Code::Unauthenticated => (v1::RejectionReason::Unauthenticated, false),
                Code::PermissionDenied => (v1::RejectionReason::Unauthorized, false),
                Code::Cancelled
                | Code::DeadlineExceeded
                | Code::ResourceExhausted
                | Code::Aborted
                | Code::Internal
                | Code::Unavailable
                | Code::Unknown => (v1::RejectionReason::Unavailable, true),
                Code::Ok
                | Code::InvalidArgument
                | Code::NotFound
                | Code::AlreadyExists
                | Code::FailedPrecondition
                | Code::OutOfRange
                | Code::Unimplemented
                | Code::DataLoss => (v1::RejectionReason::Malformed, false),
            };
            NodeReply::Refused {
                reason: reason as i32,
                retryable,
                diagnostic: format!("authority operation returned {}", status.code()),
            }
        }
        ClientError::Signing(_) => NodeReply::Refused {
            reason: v1::RejectionReason::Unauthenticated as i32,
            retryable: false,
            diagnostic: "node provider identity is unavailable".to_string(),
        },
        _ => NodeReply::Refused {
            reason: v1::RejectionReason::Unavailable as i32,
            retryable: true,
            diagnostic: "node could not complete the authority operation".to_string(),
        },
    }
}
