use std::io;

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_MESSAGE_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicIdentity {
    pub tenant_id: String,
    pub repository_id: String,
    pub node_id: String,
    pub signing_key_id: String,
    pub public_key: Vec<u8>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeRequest {
    pub version: u32,
    pub tenant_id: String,
    pub repository_id: String,
    pub expected_endpoint: Option<String>,
    pub body: Operation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Claim {
    Delegate {
        task_id: String,
        owner_id: String,
        branch: String,
        lease_seconds: u64,
        paths: Vec<String>,
        symbols: Vec<String>,
    },
    Renew {
        task_id: String,
        owner_id: String,
        lease_seconds: u64,
    },
    Release {
        task_id: String,
        owner_id: String,
    },
    Recover {
        task_id: String,
        owner_id: String,
        expected_owner: String,
        branch: String,
        lease_seconds: u64,
        paths: Vec<String>,
        symbols: Vec<String>,
        reason: String,
    },
    Park {
        task_id: String,
        owner_id: String,
    },
    Answer {
        task_id: String,
        owner_id: String,
        lease_seconds: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorScope {
    pub supervisor_id: String,
    pub session_id: String,
    pub worker_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Operation {
    Identity,
    EnrollmentStatus,
    Claim(Claim),
    ActiveClaims,
    WorkList {
        state: String,
        page: i64,
        page_size: i64,
    },
    WorkDetail {
        task_id: String,
    },
    WorkDoctor,
    ConstitutionActive,
    ConstitutionPublish {
        snapshot: Vec<u8>,
    },
    OpenSync {
        last_accepted_position: u64,
        supervisor: Option<SupervisorScope>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum NodeReply {
    Identity(PublicIdentity),
    Payload(Vec<u8>),
    SyncOpened {
        accepted_position: u64,
        enabled_capabilities: Vec<String>,
        max_in_flight_batches: u32,
        max_batch_bytes: u32,
    },
    Frame(Vec<u8>),
    Refused {
        reason: i32,
        retryable: bool,
        diagnostic: String,
    },
}

pub async fn write_message<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    message: &T,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(message).map_err(io::Error::other)?;
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "node IPC message exceeds its size limit",
        ));
    }
    writer.write_u32(bytes.len() as u32).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await
}

pub async fn read_message<R: AsyncRead + Unpin, T: DeserializeOwned>(
    reader: &mut R,
) -> io::Result<T> {
    let length = reader.read_u32().await? as usize;
    if length == 0 || length > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid node IPC message length",
        ));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid node IPC operation"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_ipc_rejects_generic_signing_and_key_destruction() {
        for value in [
            serde_json::json!({"Sign":{"domain":"anything","message_digest":[1]}}),
            serde_json::json!({"Destroy":{"key_id":"key"}}),
        ] {
            assert!(serde_json::from_value::<Operation>(value).is_err());
        }
    }

    #[tokio::test]
    async fn framing_refuses_oversized_input_before_allocating_payload() {
        let mut input = (MAX_MESSAGE_BYTES as u32 + 1)
            .to_be_bytes()
            .as_slice()
            .to_vec();
        assert!(read_message::<_, Operation>(&mut input.as_slice())
            .await
            .is_err());
        input.clear();
        assert!(
            write_message(&mut input, &NodeReply::Payload(vec![0; MAX_MESSAGE_BYTES]))
                .await
                .is_err()
        );
        assert!(input.is_empty());
    }
}
