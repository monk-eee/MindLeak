use std::{path::Path, sync::Arc};

use ackplane_node::{
    companion::NodeService, KeyHandle, NodeIdentity, NodeProcessLock, NodeSigner, NodeSignerError,
    Signature, SigningBinding,
};
use ed25519_dalek::{Signer, SigningKey};

pub struct TestCompanion {
    task: tokio::task::JoinHandle<Result<(), ackplane_client::ClientError>>,
    _lock: NodeProcessLock,
}

impl TestCompanion {
    pub async fn start(
        endpoint: &str,
        binding: SigningBinding,
        seed: &[u8; 32],
        directory: &Path,
    ) -> Self {
        std::fs::create_dir_all(directory).unwrap();
        let lock = NodeProcessLock::acquire(directory).unwrap();
        let provider = Arc::new(TestSigner {
            binding: binding.clone(),
            key: SigningKey::from_bytes(seed),
        });
        let service = Arc::new(
            NodeService::new(
                binding.tenant_id,
                binding.repository_id,
                endpoint.into(),
                provider,
            )
            .unwrap(),
        );
        service.verify_authority().await.unwrap();
        let listener = service.bind(directory).unwrap();
        let task = tokio::spawn(service.serve(listener, std::future::pending()));
        Self { task, _lock: lock }
    }
}

impl Drop for TestCompanion {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct TestSigner {
    binding: SigningBinding,
    key: SigningKey,
}

impl NodeSigner for TestSigner {
    fn identity(&self) -> NodeIdentity {
        let public_key = self.key.verifying_key().to_bytes();
        NodeIdentity {
            node_id: self.binding.node_id.clone(),
            signing_key_id: self.binding.key_id.clone(),
            fingerprint: ackplane_protocol::enrollment::public_key_fingerprint(&public_key),
            public_key,
        }
    }

    fn sign(
        &self,
        _domain: &str,
        binding: &SigningBinding,
        bytes: &[u8],
    ) -> Result<Signature, NodeSignerError> {
        if binding != &self.binding {
            return Err(NodeSignerError::BindingMismatch {
                requested: binding.clone(),
            });
        }
        Ok(Signature::from_bytes(self.key.sign(bytes).to_bytes()))
    }

    fn provision_successor(&self) -> Result<NodeIdentity, NodeSignerError> {
        Err(NodeSignerError::ProviderRefused(
            "test fixture has no successor".into(),
        ))
    }

    fn retire(&self, _handle: &KeyHandle) -> Result<(), NodeSignerError> {
        Err(NodeSignerError::ProviderRefused(
            "test fixture cannot retire keys".into(),
        ))
    }

    fn destroy(&self, _handle: &KeyHandle) -> Result<(), NodeSignerError> {
        Err(NodeSignerError::ProviderRefused(
            "test fixture cannot destroy keys".into(),
        ))
    }
}
