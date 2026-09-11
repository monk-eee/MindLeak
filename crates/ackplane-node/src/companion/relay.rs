use ackplane_client::{
    companion::wire::{read_message, write_message, NodeReply, SupervisorScope},
    ClientError, NodeSyncConnection, SigningError,
};
use ackplane_protocol::v1;
use interprocess::local_socket::tokio::Stream;
use prost::Message;
use tokio::sync::mpsc;

use super::{refused, NodeService};

impl NodeService {
    pub(super) async fn relay(
        &self,
        stream: Stream,
        mut remote: NodeSyncConnection,
        scope: Option<SupervisorScope>,
    ) -> Result<(), ClientError> {
        let (mut reader, mut writer) = tokio::io::split(stream);
        let (sender, mut pending) = mpsc::channel::<v1::NodeFrame>(16);
        let read = async {
            loop {
                let bytes: Vec<u8> = read_message(&mut reader).await?;
                let frame =
                    v1::NodeFrame::decode(bytes.as_slice()).map_err(|_| SigningError::Refused)?;
                if sender.send(frame).await.is_err() {
                    return Ok::<_, ClientError>(());
                }
            }
        };
        let exchange = async {
            let mut registered = false;
            loop {
                tokio::select! {
                    frame = pending.recv() => {
                        let Some(frame) = frame else { return Ok(()); };
                        if let Err(error) = self.validate_frame(&frame, scope.as_ref(), &mut registered).and_then(|_| self.check_key()) {
                            write_message(&mut writer, &refused(error)).await?;
                            return Ok(());
                        }
                        if let Err(error) = remote.send(frame).await {
                            write_message(&mut writer, &refused(error)).await?;
                            return Ok(());
                        }
                    }
                    reply = remote.recv() => {
                        let reply = match reply {
                            Ok(Some(reply)) => reply,
                            other => {
                                let error = other.err().unwrap_or(ClientError::HandshakeStreamClosed);
                                write_message(&mut writer, &refused(error)).await?;
                                return Ok(());
                            }
                        };
                        write_message(&mut writer, &NodeReply::Frame(reply.encode_to_vec())).await?;
                    }
                }
            }
        };
        tokio::select! { result = read => result, result = exchange => result }
    }

    fn validate_frame(
        &self,
        frame: &v1::NodeFrame,
        scope: Option<&SupervisorScope>,
        registered: &mut bool,
    ) -> Result<(), ClientError> {
        use v1::node_frame::Frame;
        let Some(scope) = scope else {
            return Err(SigningError::Refused.into());
        };
        let allowed = match frame.frame.as_ref() {
            Some(Frame::SupervisorRegistration(registration)) => {
                let matches = registration.node_id == self.binding.node_id
                    && registration.supervisor_id == scope.supervisor_id;
                if matches {
                    *registered = true;
                }
                matches
            }
            Some(Frame::SupervisorSession(session)) => {
                *registered
                    && session.supervisor_id == scope.supervisor_id
                    && session.session_id == scope.session_id
                    && session.worker_id == scope.worker_id
            }
            Some(Frame::SupervisorHeartbeat(heartbeat)) => {
                *registered && heartbeat.supervisor_id == scope.supervisor_id
            }
            Some(Frame::DirectiveReceipt(receipt)) => {
                *registered
                    && receipt.tenant_id == self.binding.tenant_id
                    && receipt.repository_id == self.binding.repository_id
                    && receipt.node_id == self.binding.node_id
                    && receipt.agent_session_id == scope.session_id
            }
            Some(Frame::ContextPacketRequest(request)) => {
                *registered && request.agent_session_id == scope.session_id
            }
            Some(Frame::ContextPacketUseReport(report)) => {
                let receipt: ackplane_protocol::context_packet::ContextPacketUseReceipt =
                    serde_json::from_slice(&report.receipt_json)
                        .map_err(|_| SigningError::Refused)?;
                *registered
                    && receipt.scope.tenant_id == self.binding.tenant_id
                    && receipt.scope.repository_id == self.binding.repository_id
                    && receipt.scope.agent_session_id == scope.session_id
            }
            Some(Frame::SupervisorLifecycleReceipt(receipt)) => {
                *registered
                    && receipt.supervisor_id == scope.supervisor_id
                    && receipt.session_id == scope.session_id
                    && receipt.worker_id == scope.worker_id
            }
            Some(Frame::Hello(_))
            | Some(Frame::ChallengeResponse(_))
            | Some(Frame::EventBatch(_))
            | Some(Frame::Heartbeat(_))
            | Some(Frame::WorkTaskCreate(_))
            | None => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(ClientError::FrameRefused {
                reason: v1::RejectionReason::Malformed,
                retryable: false,
                diagnostic: "local frame is outside the declared supervisor scope".into(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // The relay admitted receipts outside its node scope after any registration.
    #[test]
    fn relay_refuses_receipts_for_another_repository_before_forwarding() {
        let service = NodeService::new(
            "tenant".into(),
            "repository".into(),
            "http://127.0.0.1:1".into(),
            Arc::new(crate::SoftwareProvider::generate(
                "tenant",
                "repository",
                "node",
            )),
        )
        .unwrap();
        let receipt = v1::NodeFrame {
            frame: Some(v1::node_frame::Frame::DirectiveReceipt(
                v1::DirectiveReceipt {
                    tenant_id: "another-tenant".into(),
                    repository_id: "another-repository".into(),
                    node_id: "node".into(),
                    ..Default::default()
                },
            )),
        };
        let scope = SupervisorScope {
            supervisor_id: "supervisor".into(),
            session_id: "session".into(),
            worker_id: "worker".into(),
        };
        assert!(service
            .validate_frame(&receipt, Some(&scope), &mut true)
            .is_err());
        for frame in [
            v1::node_frame::Frame::Hello(Default::default()),
            v1::node_frame::Frame::ChallengeResponse(Default::default()),
            v1::node_frame::Frame::EventBatch(Default::default()),
            v1::node_frame::Frame::WorkTaskCreate(Default::default()),
        ] {
            assert!(service
                .validate_frame(
                    &v1::NodeFrame { frame: Some(frame) },
                    Some(&scope),
                    &mut true
                )
                .is_err());
        }
        let mut receipt = v1::DirectiveReceipt {
            tenant_id: "tenant".into(),
            repository_id: "repository".into(),
            node_id: "node".into(),
            agent_session_id: "session".into(),
            ..Default::default()
        };
        assert!(service
            .validate_frame(
                &v1::NodeFrame {
                    frame: Some(v1::node_frame::Frame::DirectiveReceipt(receipt.clone()))
                },
                Some(&scope),
                &mut true
            )
            .is_ok());
        receipt.agent_session_id = "other-session".into();
        assert!(service
            .validate_frame(
                &v1::NodeFrame {
                    frame: Some(v1::node_frame::Frame::DirectiveReceipt(receipt))
                },
                Some(&scope),
                &mut true
            )
            .is_err());
    }
}
