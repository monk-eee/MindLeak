use std::time::SystemTime;

use super::{KnowledgeStore, KnowledgeStoreError};
use crate::db_pool::{PgConnection, PgPool};

const MIGRATION: &str = include_str!("../../migrations/0007_knowledge.sql");
const NONCE_MIGRATION: &str =
    include_str!("../../migrations/0008_knowledge_authentication_nonces.sql");
const RECORDED_BY_MIGRATION: &str = include_str!("../../migrations/0011_knowledge_recorded_by.sql");
const RECONFIRMATION_MIGRATION: &str =
    include_str!("../../migrations/0012_knowledge_reconfirmations.sql");
const REACH_MIGRATION: &str = include_str!("../../migrations/0013_knowledge_reach.sql");
const ACTIVE_PAGE_INDEX_MIGRATION: &str =
    include_str!("../../migrations/0033_knowledge_active_page_index.sql");
const LIFECYCLE_MIGRATION: &str = include_str!("../../migrations/0034_knowledge_lifecycle.sql");
const SUPERSESSION_AND_EVIDENCE_MIGRATION: &str =
    include_str!("../../migrations/0035_knowledge_supersession_and_evidence.sql");
const REVALIDATION_POLICY_MIGRATION: &str =
    include_str!("../../migrations/0036_knowledge_revalidation_policy.sql");

impl KnowledgeStore {
    /// Takes a clone of the process's single pool (ADR-0143 decision 1), not a
    /// database URL: a store that resolved its own connection would be exactly
    /// the per-store demand the pool exists to bound.
    pub async fn connect(pool: &PgPool) -> Result<Self, KnowledgeStoreError> {
        let mut connection = pool.get().await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::KNOWLEDGE,
            MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::KNOWLEDGE_AUTHENTICATION_NONCES,
            NONCE_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::KNOWLEDGE_RECORDED_BY,
            RECORDED_BY_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::KNOWLEDGE_RECONFIRMATIONS,
            RECONFIRMATION_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::KNOWLEDGE_REACH,
            REACH_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::KNOWLEDGE_ACTIVE_PAGE_INDEX,
            ACTIVE_PAGE_INDEX_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::KNOWLEDGE_LIFECYCLE,
            LIFECYCLE_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::KNOWLEDGE_SUPERSESSION_AND_EVIDENCE,
            SUPERSESSION_AND_EVIDENCE_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::KNOWLEDGE_REVALIDATION_POLICY,
            REVALIDATION_POLICY_MIGRATION,
        )
        .await?;
        Ok(Self { pool: pool.clone() })
    }

    pub(super) async fn connection(&self) -> Result<PgConnection, KnowledgeStoreError> {
        Ok(self.pool.get().await?)
    }

    /// Resolve the signing key a knowledge request's authentication claims,
    /// judged as of now. Mirrors `ClaimStore::resolve_signing_key`: the
    /// decision itself lives in `signing_keys` and is pure, this only owns
    /// the connection.
    ///
    /// Returns `KnowledgeStoreError` rather than `SigningKeyError` because
    /// obtaining the connection is now a way this can fail, and that is the
    /// store's concern -- `signing_keys` never sees a pool.
    pub async fn resolve_signing_key(
        &self,
        binding: &crate::signing_keys::EnvelopeBinding<'_>,
    ) -> Result<crate::signing_keys::KeyResolution, KnowledgeStoreError> {
        let connection = self.connection().await?;
        Ok(crate::signing_keys::resolve(&connection, binding).await?)
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
            .connection()
            .await?
            .execute(
                "INSERT INTO knowledge_authentication_nonces (signing_key_id, nonce, consumed_at) \
                 VALUES ($1, $2, $3) ON CONFLICT (signing_key_id, nonce) DO NOTHING",
                &[&signing_key_id, &nonce, &now],
            )
            .await?;
        Ok(inserted == 1)
    }
}
