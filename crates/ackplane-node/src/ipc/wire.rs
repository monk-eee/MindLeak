//! Wire types and length-prefixed framing for the local IPC endpoint
//! (ADR-0100 decision 4). Interprocess local sockets are raw byte streams
//! with no built-in message framing, so every message here is a 4-byte
//! big-endian length prefix followed by that many bytes of JSON.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use crate::signer::{NodeIdentity, SigningBinding};

/// The maximum single message size this endpoint will read. A local,
/// repository-scoped caller has no legitimate reason to send more than this;
/// refusing early avoids letting a misbehaving caller allocate an unbounded
/// buffer.
const MAX_MESSAGE_BYTES: u32 = 1024 * 1024;

/// A wire-safe copy of [`NodeIdentity`] — every field is already public, so
/// this only exists to keep the wire schema decoupled from the in-process
/// type's exact shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeIdentityWire {
    pub node_id: String,
    pub signing_key_id: String,
    pub public_key: Vec<u8>,
    pub fingerprint: String,
}

impl From<NodeIdentity> for NodeIdentityWire {
    fn from(identity: NodeIdentity) -> Self {
        Self {
            node_id: identity.node_id,
            signing_key_id: identity.signing_key_id,
            public_key: identity.public_key.to_vec(),
            fingerprint: identity.fingerprint,
        }
    }
}

/// A wire-safe copy of [`SigningBinding`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SigningBindingWire {
    pub tenant_id: String,
    pub repository_id: String,
    pub node_id: String,
    pub key_id: String,
}

impl From<SigningBindingWire> for SigningBinding {
    fn from(wire: SigningBindingWire) -> Self {
        Self {
            tenant_id: wire.tenant_id,
            repository_id: wire.repository_id,
            node_id: wire.node_id,
            key_id: wire.key_id,
        }
    }
}

/// Every request carries the caller's declared repository id at the
/// envelope level, checked before the body is even inspected (ADR-0100
/// decision 4: "The endpoint is repository-scoped").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRequestEnvelope {
    pub repository_id: String,
    pub body: NodeRequestBody,
}

/// The closed set of operations this endpoint accepts — never arbitrary
/// bytes to sign, MCP payloads, source patches, or terminal commands
/// (ADR-0100 decision 4). `message_digest` is a digest the caller already
/// computed, matching [`crate::NodeSigner::sign`]'s own contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeRequestBody {
    Identity,
    Sign {
        domain: String,
        binding: SigningBindingWire,
        message_digest: Vec<u8>,
    },
    ProvisionSuccessor,
    Retire {
        key_id: String,
    },
    Destroy {
        key_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeResponseBody {
    Identity(NodeIdentityWire),
    Signature(Vec<u8>),
    Ok,
    Refused { reason: String },
}

pub(crate) fn write_message<W: Write>(writer: &mut W, payload: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(payload).map_err(io::Error::other)?;
    let len = u32::try_from(bytes.len()).map_err(io::Error::other)?;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}

pub(crate) fn read_message<R: Read, T: for<'de> Deserialize<'de>>(reader: &mut R) -> io::Result<T> {
    let mut len_bytes = [0u8; 4];
    reader.read_exact(&mut len_bytes)?;
    let len = u32::from_be_bytes(len_bytes);
    if len > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("message of {len} bytes exceeds the {MAX_MESSAGE_BYTES}-byte limit"),
        ));
    }
    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}
