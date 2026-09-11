use std::time::{SystemTime, UNIX_EPOCH};

use ackplane_protocol::{context_packet::*, v1};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::{
    claim_store::{ActiveClaim, ClaimStore, ClaimStoreError},
    constitution_store::{ConstitutionStore, ConstitutionStoreError},
    context_packet_compiler::ContextPacketCompilerError,
    context_packet_store::{ContextPacketStore, ContextPacketStoreError},
    db_pool::PgPool,
    directive_store::{DirectiveStore, DirectiveStoreError, MAX_DELIVERY_BATCH},
    knowledge_store::{KnowledgeStore, KnowledgeStoreError},
    projection::{ProjectionError, Projector},
    supervisor_store::{SupervisorStore, SupervisorStoreError},
    work_store::{WorkStore, WorkStoreError, WorkTaskState},
};

mod compile;
#[cfg(test)]
mod tests;

pub struct ContextService {
    packets: ContextPacketStore,
    directives: DirectiveStore,
    supervisors: SupervisorStore,
    claims: ClaimStore,
    work: WorkStore,
    constitution: ConstitutionStore,
    knowledge: KnowledgeStore,
    projection: Projector,
}

#[derive(Debug, thiserror::Error)]
pub enum ContextServiceError {
    #[error("context refused: {0}")]
    Refused(&'static str),
    #[error(transparent)]
    Directive(#[from] DirectiveStoreError),
    #[error(transparent)]
    Supervisor(#[from] SupervisorStoreError),
    #[error(transparent)]
    Claim(#[from] ClaimStoreError),
    #[error(transparent)]
    Work(#[from] WorkStoreError),
    #[error(transparent)]
    Constitution(#[from] ConstitutionStoreError),
    #[error(transparent)]
    Knowledge(#[from] KnowledgeStoreError),
    #[error(transparent)]
    Projection(#[from] ProjectionError),
    #[error(transparent)]
    Packet(#[from] ContextPacketStoreError),
    #[error(transparent)]
    Compile(#[from] ContextPacketCompilerError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl ContextService {
    pub async fn connect(pool: &PgPool) -> Result<Self, ContextServiceError> {
        Ok(Self {
            packets: ContextPacketStore::connect(pool).await?,
            directives: DirectiveStore::connect(pool).await?,
            supervisors: SupervisorStore::connect(pool).await?,
            claims: ClaimStore::connect(pool).await?,
            work: WorkStore::connect(pool).await?,
            constitution: ConstitutionStore::connect(pool).await?,
            knowledge: KnowledgeStore::connect(pool).await?,
            projection: Projector::connect(pool).await?,
        })
    }

    pub(crate) async fn handle(
        &self,
        frame: v1::NodeFrame,
        tenant_id: &str,
        repository_id: &str,
        node_id: &str,
    ) -> v1::AckplaneFrame {
        let result = match frame.frame {
            Some(v1::node_frame::Frame::ContextPacketRequest(request)) => self
                .request(tenant_id, repository_id, node_id, &request)
                .await
                .and_then(|packet| {
                    Ok(v1::ackplane_frame::Frame::ContextPacketReply(
                        v1::ContextPacketReply {
                            packet_json: serde_json::to_vec(&packet)?,
                        },
                    ))
                }),
            Some(v1::node_frame::Frame::ContextPacketUseReport(report)) => self
                .record_use(tenant_id, repository_id, node_id, &report)
                .await
                .map(|packet_id| {
                    v1::ackplane_frame::Frame::ContextPacketUseAck(v1::ContextPacketUseAck {
                        packet_id,
                    })
                }),
            _ => Err(ContextServiceError::Refused(
                "unsupported context operation",
            )),
        };
        match result {
            Ok(frame) => v1::AckplaneFrame { frame: Some(frame) },
            Err(error) => {
                let (reason, retryable, diagnostic) = match &error {
                    ContextServiceError::Refused(reason) => {
                        (v1::RejectionReason::Unauthorized, false, reason.to_string())
                    }
                    ContextServiceError::Compile(_) | ContextServiceError::Json(_) => (
                        v1::RejectionReason::Malformed,
                        false,
                        "context cannot satisfy the bounded packet contract".to_string(),
                    ),
                    _ => (
                        v1::RejectionReason::Unavailable,
                        true,
                        "context stores are unavailable".to_string(),
                    ),
                };
                tracing::warn!(%error, "context operation refused");
                v1::AckplaneFrame {
                    frame: Some(v1::ackplane_frame::Frame::Rejection(v1::Rejection {
                        record_id: String::new(),
                        reason: reason as i32,
                        retryable,
                        diagnostic,
                    })),
                }
            }
        }
    }

    async fn current_claim(
        &self,
        tenant_id: &str,
        repository_id: &str,
        task_id: &str,
        session_id: &str,
    ) -> Result<ActiveClaim, ContextServiceError> {
        let now = SystemTime::now();
        self.claims
            .list_active(tenant_id, repository_id, now)
            .await?
            .into_iter()
            .find(|claim| {
                claim.task_id == task_id
                    && claim.owner_id == session_id
                    && claim.lease_expires_at > now
            })
            .ok_or(ContextServiceError::Refused(
                "this session has no live task lease",
            ))
    }

    async fn request(
        &self,
        tenant_id: &str,
        repository_id: &str,
        node_id: &str,
        request: &v1::ContextPacketRequest,
    ) -> Result<ContextPacket, ContextServiceError> {
        if request.directive_id.len() > 256
            || request.agent_session_id.len() > 256
            || request.token_budget == 0
            || request.token_budget > 8192
        {
            return Err(ContextServiceError::Refused(
                "invalid context request bounds",
            ));
        }
        let directive = self
            .directives
            .pending_for_session(
                tenant_id,
                repository_id,
                node_id,
                &request.agent_session_id,
                MAX_DELIVERY_BATCH,
            )
            .await?
            .into_iter()
            .find(|directive| directive.directive_id == request.directive_id)
            .ok_or(ContextServiceError::Refused(
                "no pending directive for this identity and session",
            ))?;
        if !matches!(
            directive.payload,
            Some(v1::agent_directive::Payload::Assign(_))
        ) {
            return Err(ContextServiceError::Refused(
                "context requires an assignment directive",
            ));
        }
        let now = OffsetDateTime::now_utc();
        let expires = OffsetDateTime::parse(&directive.expires_at, &Rfc3339)
            .map_err(|_| ContextServiceError::Refused("invalid directive expiry"))?;
        if expires <= now {
            return Err(ContextServiceError::Refused("assignment expired"));
        }
        let task = self
            .work
            .task_detail(tenant_id, repository_id, &directive.task_id)
            .await?
            .ok_or(ContextServiceError::Refused(
                "assignment has no published task",
            ))?
            .task;
        if !matches!(task.state, WorkTaskState::Open | WorkTaskState::Claimed) {
            return Err(ContextServiceError::Refused("task is not executable"));
        }
        let claim = self
            .current_claim(
                tenant_id,
                repository_id,
                &task.task_id,
                &request.agent_session_id,
            )
            .await?;
        if claim.paths != task.declared_paths || claim.symbols != task.declared_symbols {
            return Err(ContextServiceError::Refused(
                "lease scope differs from the published task",
            ));
        }
        let constitution = self
            .constitution
            .get_active(tenant_id, repository_id)
            .await?
            .ok_or(ContextServiceError::Refused(
                "no constitution has been adopted",
            ))?;
        if !matches!(constitution.status.as_str(), "active" | "adopted") {
            return Err(ContextServiceError::Refused("constitution is not active"));
        }
        if directive.policy_refs.iter().any(|reference| {
            reference != &constitution.version_id
                && !constitution
                    .clauses
                    .iter()
                    .any(|clause| &clause.id == reference && clause.status == "active")
        }) {
            return Err(ContextServiceError::Refused(
                "assignment policy is stale or unresolved",
            ));
        }
        let packet = self
            .compile(
                &directive,
                &task,
                &claim,
                &constitution,
                request.token_budget,
                now.unix_timestamp(),
                expires.unix_timestamp(),
            )
            .await?;
        let current = self
            .constitution
            .get_active(tenant_id, repository_id)
            .await?
            .ok_or(ContextServiceError::Refused(
                "constitution changed during compilation",
            ))?;
        if current.version_id != constitution.version_id || current.version != constitution.version
        {
            return Err(ContextServiceError::Refused(
                "constitution changed during compilation",
            ));
        }
        self.current_claim(
            tenant_id,
            repository_id,
            &task.task_id,
            &request.agent_session_id,
        )
        .await?;
        self.packets.store_packet(&packet).await?;
        Ok(packet)
    }

    async fn record_use(
        &self,
        tenant_id: &str,
        repository_id: &str,
        node_id: &str,
        report: &v1::ContextPacketUseReport,
    ) -> Result<String, ContextServiceError> {
        let bytes = &report.receipt_json;
        if bytes.len() > 8192 {
            return Err(ContextServiceError::Refused(
                "context receipt exceeds its bound",
            ));
        }
        let receipt: ContextPacketUseReceipt = serde_json::from_slice(bytes)?;
        if receipt.scope.tenant_id != tenant_id || receipt.scope.repository_id != repository_id {
            return Err(ContextServiceError::Refused(
                "context receipt does not belong to this identity",
            ));
        }
        let session = self
            .supervisors
            .session(tenant_id, repository_id, &receipt.scope.agent_session_id)
            .await?
            .ok_or(ContextServiceError::Refused(
                "context receipt has no registered session",
            ))?;
        if !self
            .supervisors
            .list_supervisors(tenant_id, repository_id)
            .await?
            .iter()
            .any(|status| {
                status.registration.supervisor_id == session.session.supervisor_id
                    && status.registration.identity.node_id == node_id
            })
        {
            return Err(ContextServiceError::Refused(
                "context receipt does not belong to this node",
            ));
        }
        self.packets.record_use(&receipt).await?;
        if let Some(sequence) = report.outbox_sequence {
            self.supervisors
                .record_outbox_sequence(
                    tenant_id,
                    repository_id,
                    &session.session.supervisor_id,
                    sequence,
                )
                .await?;
        }
        Ok(receipt.packet_id)
    }
}

fn unix_seconds(time: SystemTime) -> Result<i64, ContextServiceError> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .ok_or(ContextServiceError::Refused("invalid source clock"))
}
