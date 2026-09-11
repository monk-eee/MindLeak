use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use interprocess::local_socket::{
    tokio::{prelude::*, Stream},
    Name,
};
use prost::Message;

use crate::{ClientError, NodeSyncConnection};
use ackplane_protocol::v1;

pub mod wire;
use wire::{read_message, write_message, NodeReply, NodeRequest, Operation, PublicIdentity};

pub const STATE_DIR_ENV: &str = "MINDLEAK_ACKPLANE_STATE_DIR";
pub const CALL_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeClient {
    pub state_dir: PathBuf,
    pub tenant_id: String,
    pub repository_id: String,
    pub expected_endpoint: Option<String>,
}

impl NodeClient {
    pub fn new(state_dir: PathBuf, tenant_id: String, repository_id: String) -> Self {
        Self {
            state_dir,
            tenant_id,
            repository_id,
            expected_endpoint: None,
        }
    }

    pub async fn request(&self, operation: Operation) -> Result<NodeReply, ClientError> {
        tokio::time::timeout(CALL_TIMEOUT, async {
            let mut stream = self.connect(operation).await?;
            checked(read_message(&mut stream).await?)
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "node companion request timed out"))?
    }

    pub async fn identity(&self) -> Result<PublicIdentity, ClientError> {
        match self.request(Operation::Identity).await? {
            NodeReply::Identity(identity)
                if identity.tenant_id == self.tenant_id
                    && identity.repository_id == self.repository_id =>
            {
                Ok(identity)
            }
            _ => Err(protocol_error()),
        }
    }

    pub async fn status(&self) -> Result<v1::EnrollmentStatusResult, ClientError> {
        self.protobuf(Operation::EnrollmentStatus).await
    }

    pub async fn protobuf<T: Message + Default>(
        &self,
        operation: Operation,
    ) -> Result<T, ClientError> {
        match self.request(operation).await? {
            NodeReply::Payload(bytes) => T::decode(bytes.as_slice()).map_err(|_| protocol_error()),
            _ => Err(protocol_error()),
        }
    }

    pub async fn open_sync(
        &self,
        last_accepted_position: u64,
        supervisor: Option<wire::SupervisorScope>,
    ) -> Result<NodeSyncConnection, ClientError> {
        tokio::time::timeout(CALL_TIMEOUT, async {
            let mut stream = self
                .connect(Operation::OpenSync {
                    last_accepted_position,
                    supervisor,
                })
                .await?;
            match checked(read_message(&mut stream).await?)? {
                NodeReply::SyncOpened {
                    accepted_position,
                    enabled_capabilities,
                    max_in_flight_batches,
                    max_batch_bytes,
                } => Ok(NodeSyncConnection::from_companion(
                    stream,
                    accepted_position,
                    enabled_capabilities,
                    v1::FlowControl {
                        max_in_flight_batches,
                        max_batch_bytes,
                    },
                )),
                _ => Err(protocol_error()),
            }
        })
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "node companion handshake timed out",
            )
        })?
    }

    async fn connect(&self, body: Operation) -> Result<Stream, ClientError> {
        let mut stream = Stream::connect(endpoint_name(&self.state_dir)?).await?;
        write_message(
            &mut stream,
            &NodeRequest {
                version: 1,
                tenant_id: self.tenant_id.clone(),
                repository_id: self.repository_id.clone(),
                expected_endpoint: self.expected_endpoint.clone(),
                body,
            },
        )
        .await?;
        Ok(stream)
    }
}

pub fn endpoint_name(directory: &Path) -> io::Result<Name<'static>> {
    if !directory.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "node state directory must be absolute",
        ));
    }
    let directory = directory.canonicalize()?;
    #[cfg(unix)]
    {
        use interprocess::local_socket::{GenericFilePath, ToFsName};
        directory
            .join("ackplane-node.sock")
            .to_fs_name::<GenericFilePath>()
    }
    #[cfg(windows)]
    {
        use interprocess::local_socket::{GenericNamespaced, ToNsName};
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(directory.to_string_lossy().to_lowercase().as_bytes());
        let suffix = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("mindleak-node-{suffix}").to_ns_name::<GenericNamespaced>()
    }
}

pub(crate) fn checked(reply: NodeReply) -> Result<NodeReply, ClientError> {
    match reply {
        NodeReply::Refused {
            reason,
            retryable,
            diagnostic,
        } => Err(ClientError::ConnectionRefused {
            reason: v1::RejectionReason::try_from(reason)
                .unwrap_or(v1::RejectionReason::Unspecified),
            retryable,
            diagnostic,
        }),
        other => Ok(other),
    }
}

pub(crate) fn protocol_error() -> ClientError {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "unexpected node companion response",
    )
    .into()
}
