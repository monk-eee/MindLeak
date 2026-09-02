//! The local IPC endpoint (ADR-0100 decision 4): a Windows named pipe or a
//! Unix-domain socket under the repository's own state directory, scoped to
//! one repository id, accepting only the closed [`crate::NodeSigner`]
//! operations — never a TCP listener, never a reusable bearer token, and
//! never arbitrary bytes to sign.

pub mod wire;

use std::io;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

use interprocess::local_socket::prelude::*;
#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::{ListenerOptions, Stream};

use crate::signer::KeyHandle;
use crate::NodeSigner;
use wire::{read_message, write_message, NodeRequestBody, NodeRequestEnvelope, NodeResponseBody};

/// A repository-scoped local IPC endpoint. Binds a Windows named pipe or a
/// Unix-domain socket, never a TCP listener.
pub struct NodeIpcListener {
    inner: interprocess::local_socket::Listener,
    repository_id: String,
}

impl NodeIpcListener {
    /// Binds the endpoint for `repository_id`. On Unix this creates a real
    /// socket file under `repository_state_dir`, restricted to owner-only
    /// (0600) permissions once bound; on Windows it opens a named pipe
    /// scoped to this repository id (named pipes are OS-namespaced, not
    /// filesystem paths, so `repository_state_dir` is unused there).
    pub fn bind(repository_id: &str, repository_state_dir: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(repository_state_dir)?;
        let name = endpoint_name(repository_id, repository_state_dir)?;
        let inner = ListenerOptions::new().name(name).create_sync()?;
        #[cfg(unix)]
        restrict_to_owner(repository_id, repository_state_dir)?;
        Ok(Self {
            inner,
            repository_id: repository_id.to_string(),
        })
    }

    /// Accepts and fully services exactly one request, then returns. A real
    /// daemon loop calls this repeatedly; tests call it once per connection
    /// to keep control flow explicit.
    pub fn accept_one(&self, signer: &dyn NodeSigner) -> io::Result<()> {
        let conn = self.inner.accept()?;
        serve_one(conn, &self.repository_id, signer)
    }
}

fn serve_one(
    mut conn: Stream,
    expected_repository_id: &str,
    signer: &dyn NodeSigner,
) -> io::Result<()> {
    let envelope: NodeRequestEnvelope = read_message(&mut conn)?;
    if envelope.repository_id != expected_repository_id {
        return write_message(
            &mut conn,
            &NodeResponseBody::Refused {
                reason: format!(
                    "repository id {:?} does not match this endpoint's repository {:?}",
                    envelope.repository_id, expected_repository_id
                ),
            },
        );
    }
    let response = handle_body(envelope.body, signer);
    write_message(&mut conn, &response)
}

fn handle_body(body: NodeRequestBody, signer: &dyn NodeSigner) -> NodeResponseBody {
    match body {
        NodeRequestBody::Identity => NodeResponseBody::Identity(signer.identity().into()),
        NodeRequestBody::Sign {
            domain,
            binding,
            message_digest,
        } => match signer.sign(&domain, &binding.into(), &message_digest) {
            Ok(signature) => NodeResponseBody::Signature(signature.as_bytes().to_vec()),
            Err(err) => NodeResponseBody::Refused {
                reason: err.to_string(),
            },
        },
        NodeRequestBody::ProvisionSuccessor => match signer.provision_successor() {
            Ok(identity) => NodeResponseBody::Identity(identity.into()),
            Err(err) => NodeResponseBody::Refused {
                reason: err.to_string(),
            },
        },
        NodeRequestBody::Retire { key_id } => {
            match signer.retire(&KeyHandle::from_signing_key_id(key_id)) {
                Ok(()) => NodeResponseBody::Ok,
                Err(err) => NodeResponseBody::Refused {
                    reason: err.to_string(),
                },
            }
        }
        NodeRequestBody::Destroy { key_id } => {
            match signer.destroy(&KeyHandle::from_signing_key_id(key_id)) {
                Ok(()) => NodeResponseBody::Ok,
                Err(err) => NodeResponseBody::Refused {
                    reason: err.to_string(),
                },
            }
        }
    }
}

/// Connects to the endpoint bound for `endpoint_repository_id` and sends one
/// request declaring `declared_repository_id` (normally the same value; a
/// mismatch is how a misdirected or confused caller gets refused rather than
/// silently serviced).
pub fn connect_and_send(
    endpoint_repository_id: &str,
    repository_state_dir: &Path,
    declared_repository_id: &str,
    body: NodeRequestBody,
) -> io::Result<NodeResponseBody> {
    let name = endpoint_name(endpoint_repository_id, repository_state_dir)?;
    let mut conn = Stream::connect(name)?;
    write_message(
        &mut conn,
        &NodeRequestEnvelope {
            repository_id: declared_repository_id.to_string(),
            body,
        },
    )?;
    read_message(&mut conn)
}

#[cfg(windows)]
fn endpoint_name(
    repository_id: &str,
    _repository_state_dir: &Path,
) -> io::Result<interprocess::local_socket::Name<'static>> {
    format!("mindleak-ackplane-node-{repository_id}").to_ns_name::<GenericNamespaced>()
}

#[cfg(unix)]
fn endpoint_name(
    repository_id: &str,
    repository_state_dir: &Path,
) -> io::Result<interprocess::local_socket::Name<'static>> {
    socket_path(repository_id, repository_state_dir).to_fs_name::<GenericFilePath>()
}

#[cfg(unix)]
fn socket_path(repository_id: &str, repository_state_dir: &Path) -> PathBuf {
    repository_state_dir.join(format!("ackplane-node-{repository_id}.sock"))
}

#[cfg(unix)]
fn restrict_to_owner(repository_id: &str, repository_state_dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let path = socket_path(repository_id, repository_state_dir);
    let mut perms = std::fs::metadata(&path)?.permissions();
    perms.set_mode(0o600);
    std::fs::set_permissions(&path, perms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::software::SoftwareProvider;

    #[test]
    fn a_correctly_scoped_client_receives_an_identity_response() {
        let dir = tempfile::tempdir().unwrap();
        let signer = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        let listener = NodeIpcListener::bind("repo-a", dir.path()).unwrap();

        let server = std::thread::spawn(move || listener.accept_one(&signer));

        let response =
            connect_and_send("repo-a", dir.path(), "repo-a", NodeRequestBody::Identity).unwrap();

        server.join().unwrap().unwrap();
        assert!(matches!(response, NodeResponseBody::Identity(_)));
    }

    #[test]
    fn a_request_declaring_a_different_repository_id_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let signer = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        let listener = NodeIpcListener::bind("repo-a", dir.path()).unwrap();

        let server = std::thread::spawn(move || listener.accept_one(&signer));

        // Connects to the real "repo-a" endpoint but declares a different
        // repository id inside the envelope -- the protocol-level check,
        // independent of which physical endpoint was reachable at all.
        let response =
            connect_and_send("repo-a", dir.path(), "repo-b", NodeRequestBody::Identity).unwrap();

        server.join().unwrap().unwrap();
        assert!(matches!(response, NodeResponseBody::Refused { .. }));
    }
}
