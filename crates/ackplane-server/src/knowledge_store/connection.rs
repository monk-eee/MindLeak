use std::time::SystemTime;

use tokio_postgres::NoTls;

use super::{KnowledgeStore, KnowledgeStoreError};

const MIGRATION: &str = include_str!("../../migrations/0007_knowledge.sql");
const NONCE_MIGRATION: &str =
    include_str!("../../migrations/0008_knowledge_authentication_nonces.sql");
const RECORDED_BY_MIGRATION: &str = include_str!("../../migrations/0011_knowledge_recorded_by.sql");
const RECONFIRMATION_MIGRATION: &str =
    include_str!("../../migrations/0012_knowledge_reconfirmations.sql");
const REACH_MIGRATION: &str = include_str!("../../migrations/0013_knowledge_reach.sql");
const ACTIVE_PAGE_INDEX_MIGRATION: &str =
    include_str!("../../migrations/0033_knowledge_active_page_index.sql");

impl KnowledgeStore {
    pub async fn connect(database_url: &str) -> Result<Self, tokio_postgres::Error> {
        let (mut client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!(%error, "ackplane knowledge store connection closed with an error");
            }
        });
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::KNOWLEDGE,
            MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::KNOWLEDGE_AUTHENTICATION_NONCES,
            NONCE_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::KNOWLEDGE_RECORDED_BY,
            RECORDED_BY_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::KNOWLEDGE_RECONFIRMATIONS,
            RECONFIRMATION_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::KNOWLEDGE_REACH,
            REACH_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::KNOWLEDGE_ACTIVE_PAGE_INDEX,
            ACTIVE_PAGE_INDEX_MIGRATION,
        )
        .await?;
        Ok(Self { client })
    }

    /// Resolve the signing key a knowledge request's authentication claims,
    /// judged as of now. Mirrors `ClaimStore::resolve_signing_key`: the
    /// decision itself lives in `signing_keys` and is pure, this only owns
    /// the connection.
    pub async fn resolve_signing_key(
        &self,
        binding: &crate::signing_keys::EnvelopeBinding<'_>,
    ) -> Result<crate::signing_keys::KeyResolution, crate::signing_keys::SigningKeyError> {
        crate::signing_keys::resolve(&self.client, binding).await
    }

    /// Consume a (signing_key_id, nonce) pair exactly once (anti-replay for
    /// `KnowledgeGrpcService` authentication, ADR-0108 decision 3). Returns
    /// true the first time a pair is seen, false on every later attempt with
    /// the identical pair -- the insert's own uniqueness is the enforcement,
    /// so this needs no read-then-write race.
    pub async fn consume_knowledge_nonce(
        &self,
        signing_key_id: &str,
        nonce: &[u8],
        now: SystemTime,
    ) -> Result<bool, KnowledgeStoreError> {
        let inserted = self
            .client
            .execute(
                "INSERT INTO knowledge_authentication_nonces (signing_key_id, nonce, consumed_at) \
                 VALUES ($1, $2, $3) ON CONFLICT (signing_key_id, nonce) DO NOTHING",
                &[&signing_key_id, &nonce, &now],
            )
            .await?;
        Ok(inserted == 1)
    }
}
