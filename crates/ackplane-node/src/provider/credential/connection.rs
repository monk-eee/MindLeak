use ackplane_client::{ClaimSigner, ClientError, NodeSyncConnection, SigningError};

use super::{CredentialProvider, CredentialProviderError};

impl CredentialProvider {
    /// Authenticate a NodeSync connection with this provider's recorded identity.
    /// The caller retains the provider as the repository's credential owner.
    #[allow(
        clippy::result_large_err,
        reason = "preserve the shared client's tonic::Status-bearing error contract"
    )]
    pub async fn open_connection(
        &self,
        endpoint: &str,
        capabilities: Vec<String>,
        last_accepted_position: u64,
    ) -> Result<NodeSyncConnection, ClientError> {
        NodeSyncConnection::open(
            endpoint,
            &ConnectionSigner(self),
            &self.binding.tenant_id,
            &self.binding.repository_id,
            capabilities,
            last_accepted_position,
        )
        .await
    }
}

struct ConnectionSigner<'provider>(&'provider CredentialProvider);

impl ClaimSigner for ConnectionSigner<'_> {
    fn signing_key_id(&self) -> &str {
        &self.0.identity.signing_key_id
    }

    fn node_id(&self) -> &str {
        &self.0.identity.node_id
    }

    fn sign(&self, bytes: &[u8]) -> Result<Vec<u8>, SigningError> {
        self.0
            .storage
            .sign(bytes)
            .map(|signature| signature.as_bytes().to_vec())
            .map_err(|error| match error {
                CredentialProviderError::Facility(_) | CredentialProviderError::Random => {
                    SigningError::Unavailable
                }
                CredentialProviderError::InvalidBinding
                | CredentialProviderError::InvalidProvider
                | CredentialProviderError::MalformedCredential
                | CredentialProviderError::IdentityMismatch => SigningError::IdentityMismatch,
                CredentialProviderError::Lock(_)
                | CredentialProviderError::Enrollment(_)
                | CredentialProviderError::AlreadyProvisioned
                | CredentialProviderError::NotActivated
                | CredentialProviderError::AlreadyActivated
                | CredentialProviderError::InvalidChallenge
                | CredentialProviderError::NoChallenge
                | CredentialProviderError::InvalidActivation => SigningError::Refused,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{binding, credential};
    use super::*;

    // Erasing a signer without Sync made the connection future unusable on runtime workers.
    #[test]
    fn the_connection_future_can_be_sent_to_a_runtime_worker() {
        let directory = tempfile::tempdir().unwrap();
        let entry = credential();
        let provider =
            CredentialProvider::provision_with(binding(), directory.path(), |_| Ok(entry.clone()))
                .unwrap();
        let connection = provider.open_connection("http://127.0.0.1:1", Vec::new(), 0);
        let _: &dyn Send = &connection;
    }

    #[test]
    fn the_client_adapter_rechecks_credentials_and_returns_only_safe_errors() {
        let directory = tempfile::tempdir().unwrap();
        let entry = credential();
        let provider =
            CredentialProvider::provision_with(binding(), directory.path(), |_| Ok(entry.clone()))
                .unwrap();
        let signer = ConnectionSigner(&provider);
        assert_eq!(signer.node_id(), binding().node_id);
        assert_eq!(signer.signing_key_id(), binding().key_id);
        assert!(signer.sign(b"client-challenge").is_ok());

        let marker = "private-provider-error-must-not-be-returned";
        entry
            .get_credential()
            .downcast_ref::<keyring::mock::MockCredential>()
            .unwrap()
            .set_error(keyring::Error::BadEncoding(marker.as_bytes().to_vec()));
        let error = signer.sign(b"client-challenge").unwrap_err();
        assert_eq!(error, SigningError::Unavailable);
        assert!(!error.to_string().contains(marker));
        assert!(!format!("{error:?}").contains(marker));

        let mut replaced: serde_json::Value =
            serde_json::from_str(&entry.get_password().unwrap()).unwrap();
        replaced["seed"] = serde_json::json!([42_u8; 32].as_slice());
        entry
            .set_password(&serde_json::to_string(&replaced).unwrap())
            .unwrap();
        assert_eq!(
            signer.sign(b"client-challenge"),
            Err(SigningError::IdentityMismatch)
        );
        entry.set_password("malformed-credential").unwrap();
        assert_eq!(
            signer.sign(b"client-challenge"),
            Err(SigningError::IdentityMismatch)
        );
        entry.delete_password().unwrap();
        assert_eq!(
            signer.sign(b"client-challenge"),
            Err(SigningError::Unavailable)
        );
    }
}
