use super::{ProjectionError, Projector};
use crate::signing_keys::{self, EnvelopeBinding, KeyResolution};
use std::time::SystemTime;

impl Projector {
    pub(crate) async fn resolve_signing_key(
        &self,
        binding: &EnvelopeBinding<'_>,
    ) -> Result<KeyResolution, ProjectionError> {
        let connection = self.connection().await?;
        Ok(signing_keys::resolve(&connection, binding).await?)
    }

    pub(crate) async fn consume_embedding_nonce(
        &self,
        signing_key_id: &str,
        nonce: &[u8],
        now: SystemTime,
    ) -> Result<bool, ProjectionError> {
        let inserted = self
            .connection()
            .await?
            .execute(
                "INSERT INTO projection_embedding_authentication_nonces \
             (signing_key_id, nonce, consumed_at) VALUES ($1, $2, $3) \
             ON CONFLICT (signing_key_id, nonce) DO NOTHING",
                &[&signing_key_id, &nonce, &now],
            )
            .await?;
        Ok(inserted == 1)
    }
}
