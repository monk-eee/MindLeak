#![allow(
    clippy::result_large_err,
    reason = "preserve the shared client's tonic::Status-bearing error contract"
)]

use std::{future::Future, io, path::Path, sync::Arc, time::Duration};

use ackplane_client::{
    companion::wire::{
        read_message, write_message, NodeReply, NodeRequest, Operation, PublicIdentity,
    },
    ClaimSigner, ClientError, NodeSyncConnection, SigningError,
};
use ackplane_protocol::v1;
use interprocess::local_socket::tokio::{prelude::*, Listener, Stream};
use tokio::{
    sync::{watch, Semaphore},
    task::JoinSet,
};

use crate::{NodeSigner, SigningBinding};

mod constitution;
mod endpoint;
mod operations;
mod relay;
mod requests;
use requests::refused;
#[cfg(windows)]
mod windows;

pub struct NodeService {
    identity: PublicIdentity,
    binding: SigningBinding,
    signer: Arc<dyn NodeSigner>,
    endpoint: String,
    streams: Semaphore,
}

impl NodeService {
    pub fn new(
        tenant_id: String,
        repository_id: String,
        endpoint: String,
        signer: Arc<dyn NodeSigner>,
    ) -> Result<Self, ClientError> {
        let identity = signer.identity();
        if [
            tenant_id.as_str(),
            repository_id.as_str(),
            &identity.node_id,
            &identity.signing_key_id,
        ]
        .iter()
        .any(|value| value.trim().is_empty())
        {
            return Err(SigningError::IdentityMismatch.into());
        }
        let binding = SigningBinding {
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            node_id: identity.node_id.clone(),
            key_id: identity.signing_key_id.clone(),
        };
        Ok(Self {
            identity: PublicIdentity {
                tenant_id,
                repository_id,
                node_id: identity.node_id,
                signing_key_id: identity.signing_key_id,
                public_key: identity.public_key.to_vec(),
                fingerprint: identity.fingerprint,
            },
            binding,
            signer,
            endpoint,
            streams: Semaphore::new(32),
        })
    }

    /// The caller must retain the provider's repository process lock for this listener's lifetime.
    pub fn bind(&self, directory: &Path) -> io::Result<Listener> {
        endpoint::bind(directory)
    }

    pub async fn verify_authority(&self) -> Result<(), ClientError> {
        self.check_key()?;
        let connection = tokio::time::timeout(Duration::from_secs(5), self.open(0))
            .await
            .map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "node authority check timed out")
            })??;
        drop(connection);
        Ok(())
    }

    pub async fn serve(
        self: Arc<Self>,
        listener: Listener,
        shutdown: impl Future<Output = ()>,
    ) -> Result<(), ClientError> {
        let permits = Arc::new(Semaphore::new(64));
        let (cancel, _) = watch::channel(false);
        let mut clients = JoinSet::new();
        let mut health = tokio::time::interval(Duration::from_secs(1));
        health.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        tokio::pin!(shutdown);
        let result = loop {
            tokio::select! {
                _ = &mut shutdown => break Ok(()),
                _ = health.tick() => {
                    if let Err(error) = self.verify_authority().await { break Err(error); }
                }
                accepted = listener.accept() => {
                    let stream = match accepted { Ok(stream) => stream, Err(error) => break Err(error.into()) };
                    if let Ok(permit) = permits.clone().try_acquire_owned() {
                        let service = self.clone();
                        let stopped = cancel.subscribe();
                        clients.spawn(async move { let _permit = permit; service.handle(stream, stopped).await });
                    }
                }
                _ = clients.join_next(), if !clients.is_empty() => {}
            }
        };
        cancel.send_replace(true);
        clients.shutdown().await;
        result
    }

    fn check_key(&self) -> Result<(), ClientError> {
        if self.signer.identity().public_key.as_slice() != self.identity.public_key.as_slice() {
            return Err(SigningError::IdentityMismatch.into());
        }
        self.signer
            .sign("node.health", &self.binding, b"mindleak.node.health.v1")
            .map_err(|_| ClientError::Signing(SigningError::Unavailable))?;
        Ok(())
    }

    async fn open(&self, position: u64) -> Result<NodeSyncConnection, ClientError> {
        NodeSyncConnection::open(
            &self.endpoint,
            &ServiceSigner(self),
            &self.binding.tenant_id,
            &self.binding.repository_id,
            vec!["synchronize".to_string()],
            position,
        )
        .await
    }

    async fn handle(
        &self,
        mut stream: Stream,
        mut stopped: watch::Receiver<bool>,
    ) -> Result<(), ClientError> {
        let request = tokio::time::timeout(
            Duration::from_secs(5),
            read_message::<_, NodeRequest>(&mut stream),
        )
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "node IPC request timed out"))??;
        if request.version != 1
            || request.tenant_id != self.binding.tenant_id
            || request.repository_id != self.binding.repository_id
        {
            write_message(&mut stream, &refused(SigningError::IdentityMismatch.into())).await?;
            return Ok(());
        }
        if let Err(error) = self.check_key() {
            write_message(&mut stream, &refused(error)).await?;
            return Ok(());
        }
        if request
            .expected_endpoint
            .as_ref()
            .is_some_and(|expected| expected != &self.endpoint)
        {
            write_message(
                &mut stream,
                &NodeReply::Refused {
                    reason: v1::RejectionReason::Malformed as i32,
                    retryable: false,
                    diagnostic: "node companion endpoint does not match the requested authority"
                        .into(),
                },
            )
            .await?;
            return Ok(());
        }
        let operation = async {
            if let Operation::OpenSync {
                last_accepted_position,
                supervisor,
            } = request.body
            {
                let Ok(_stream_permit) = self.streams.try_acquire() else {
                    write_message(
                        &mut stream,
                        &NodeReply::Refused {
                            reason: v1::RejectionReason::Unavailable as i32,
                            retryable: true,
                            diagnostic: "node companion stream limit reached".into(),
                        },
                    )
                    .await?;
                    return Ok(());
                };
                if supervisor.as_ref().is_some_and(|scope| {
                    [&scope.supervisor_id, &scope.session_id, &scope.worker_id]
                        .iter()
                        .any(|id| id.trim().is_empty() || id.len() > 256)
                }) {
                    write_message(&mut stream, &refused(SigningError::Refused.into())).await?;
                    return Ok(());
                }
                let connection = match self.open(last_accepted_position).await {
                    Ok(connection) => connection,
                    Err(error) => {
                        write_message(&mut stream, &refused(error)).await?;
                        return Ok(());
                    }
                };
                write_message(
                    &mut stream,
                    &NodeReply::SyncOpened {
                        accepted_position: connection.accepted_position(),
                        enabled_capabilities: connection.enabled_capabilities().to_vec(),
                        max_in_flight_batches: connection.flow_control().max_in_flight_batches,
                        max_batch_bytes: connection.flow_control().max_batch_bytes,
                    },
                )
                .await?;
                self.relay(stream, connection, supervisor).await
            } else {
                let reply =
                    match tokio::time::timeout(Duration::from_secs(8), self.dispatch(request.body))
                        .await
                    {
                        Ok(Ok(reply)) => reply,
                        Ok(Err(error)) => refused(error),
                        Err(_) => refused(
                            io::Error::new(io::ErrorKind::TimedOut, "node operation timed out")
                                .into(),
                        ),
                    };
                write_message(&mut stream, &reply).await?;
                Ok(())
            }
        };
        tokio::select! { result = operation => result, _ = stopped.changed() => Ok(()) }
    }
}

struct ServiceSigner<'service>(&'service NodeService);
impl ClaimSigner for ServiceSigner<'_> {
    fn node_id(&self) -> &str {
        &self.0.binding.node_id
    }
    fn signing_key_id(&self) -> &str {
        &self.0.binding.key_id
    }
    fn sign(&self, bytes: &[u8]) -> Result<Vec<u8>, SigningError> {
        self.0
            .signer
            .sign("ackplane.operation", &self.0.binding, bytes)
            .map(|signature| signature.as_bytes().to_vec())
            .map_err(|_| SigningError::Unavailable)
    }
}

#[cfg(test)]
mod tests;
