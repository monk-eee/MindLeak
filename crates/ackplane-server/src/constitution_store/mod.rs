//! Ackplane's PostgreSQL-backed read-only projection of a repository's own
//! authoritative local Lodestar constitution (ADR-0106 decision 3).
//!
//! A snapshot replaces the prior one for that tenant/repository wholesale --
//! this is a projection of the local constitution, never a second source of
//! truth for it, the same rule `knowledge`/`fleet` already follow for their
//! own domains. No adopt/tailor/reject/promote/waiver action lives here.
//!
//! Split along the same line the store's own domains follow: this file
//! keeps the mutable active-snapshot path (`publish`/`get_active`); the
//! immutable publication-history path (decision 1) lives in
//! [`publication_history`].

mod proposals;
mod publication_history;

pub use proposals::{ConstitutionProposal, ProposeConstitutionClauseRequest};
pub use publication_history::{ConstitutionPublication, RecordConstitutionPublicationRequest};

use std::time::SystemTime;

use tokio_postgres::{Client, NoTls};

const MIGRATION: &str = include_str!("../../migrations/0009_constitution.sql");
const NONCE_MIGRATION: &str =
    include_str!("../../migrations/0010_constitution_authentication_nonces.sql");
const PUBLICATION_HISTORY_MIGRATION: &str =
    include_str!("../../migrations/0026_constitution_publication_history.sql");
const PROPOSALS_MIGRATION: &str = include_str!("../../migrations/0038_constitution_proposals.sql");
const DISPLAY_LABEL_MIGRATION: &str =
    include_str!("../../migrations/0059_design_constitution_display_label.sql");

#[derive(Debug, thiserror::Error)]
pub enum ConstitutionStoreError {
    #[error("constitution database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("version_id must not be empty")]
    EmptyVersionId,
    #[error("constitution publication schema_version must not be empty")]
    EmptySchemaVersion,
    #[error("constitution publication status must not be empty")]
    EmptyStatus,
    #[error(
        "constitution publication {version_id} is already recorded with different content -- publications are immutable"
    )]
    PublicationImmutabilityViolation { version_id: String },
    #[error("stored constitution publication payload for {version_id} is unreadable: {detail}")]
    CorruptPublicationPayload { version_id: String, detail: String },
    #[error("proposal_id must not be empty")]
    EmptyProposalId,
    #[error("constitution proposal author must not be empty")]
    EmptyAuthor,
    #[error(
        "constitution proposal {proposal_id} is already recorded with different content -- proposals are immutable"
    )]
    ProposalImmutabilityViolation { proposal_id: String },
    #[error(
        "constitution proposal {proposal_id} was withdrawn and cannot be re-proposed under the same identity -- withdrawal is terminal"
    )]
    ProposalWithdrawn { proposal_id: String },
}

/// One clause, as both `publish` accepts it and `get_active` returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClauseSnapshot {
    pub id: String,
    pub slug: String,
    pub kind: String,
    pub title: String,
    pub statement: String,
    pub status: String,
    pub consequence: Option<String>,
    pub scope: Option<String>,
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PublishConstitutionRequest {
    pub tenant_id: String,
    pub repository_id: String,
    pub version_id: String,
    pub version: i64,
    pub status: String,
    pub clauses: Vec<ClauseSnapshot>,
}

/// The active snapshot as `get_active` returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveConstitution {
    pub version_id: String,
    pub version: i64,
    pub status: String,
    pub clauses: Vec<ClauseSnapshot>,
    pub published_at: SystemTime,
}

pub struct ConstitutionStore {
    client: Client,
}

impl ConstitutionStore {
    pub async fn connect(database_url: &str) -> Result<Self, tokio_postgres::Error> {
        let (mut client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!(%error, "ackplane constitution store connection closed with an error");
            }
        });
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::CONSTITUTION,
            MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::CONSTITUTION_AUTHENTICATION_NONCES,
            NONCE_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::CONSTITUTION_PUBLICATION_HISTORY,
            PUBLICATION_HISTORY_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::CONSTITUTION_PROPOSALS,
            PROPOSALS_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::DESIGN_CONSTITUTION_DISPLAY_LABEL,
            DISPLAY_LABEL_MIGRATION,
        )
        .await?;
        Ok(Self { client })
    }

    /// Resolve the signing key a constitution request's authentication
    /// claims, judged as of now. Mirrors `KnowledgeStore::resolve_signing_key`:
    /// the decision itself lives in `signing_keys` and is pure, this only
    /// owns the connection.
    pub async fn resolve_signing_key(
        &self,
        binding: &crate::signing_keys::EnvelopeBinding<'_>,
    ) -> Result<crate::signing_keys::KeyResolution, crate::signing_keys::SigningKeyError> {
        crate::signing_keys::resolve(&self.client, binding).await
    }

    /// Consume a (signing_key_id, nonce) pair exactly once (anti-replay for
    /// `ConstitutionGrpcService` authentication). Returns true the first time
    /// a pair is seen, false on every later attempt with the identical pair
    /// -- the insert's own uniqueness is the enforcement, so this needs no
    /// read-then-write race.
    pub async fn consume_constitution_nonce(
        &self,
        signing_key_id: &str,
        nonce: &[u8],
        now: SystemTime,
    ) -> Result<bool, ConstitutionStoreError> {
        let inserted = self
            .client
            .execute(
                "INSERT INTO constitution_authentication_nonces (signing_key_id, nonce, consumed_at) \
                 VALUES ($1, $2, $3) ON CONFLICT (signing_key_id, nonce) DO NOTHING",
                &[&signing_key_id, &nonce, &now],
            )
            .await?;
        Ok(inserted == 1)
    }

    /// Replace the tenant/repository's snapshot wholesale: upsert the
    /// version row, then delete and reinsert every clause. A constitution
    /// version is immutable at the source, so a publish is always a
    /// complete new snapshot, not an incremental diff this projection would
    /// otherwise have to reconcile clause-by-clause.
    pub async fn publish(
        &mut self,
        request: PublishConstitutionRequest,
    ) -> Result<SystemTime, ConstitutionStoreError> {
        if request.version_id.trim().is_empty() {
            return Err(ConstitutionStoreError::EmptyVersionId);
        }
        let published_at = SystemTime::now();
        let transaction = self.client.transaction().await?;
        transaction
            .execute(
                "INSERT INTO constitution_snapshots \
                 (tenant_id, repository_id, version_id, version, status, published_at) \
                 VALUES ($1, $2, $3, $4, $5, $6) \
                 ON CONFLICT (tenant_id, repository_id) DO UPDATE SET \
                     version_id = EXCLUDED.version_id, \
                     version = EXCLUDED.version, \
                     status = EXCLUDED.status, \
                     published_at = EXCLUDED.published_at",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.version_id,
                    &request.version,
                    &request.status,
                    &published_at,
                ],
            )
            .await?;
        transaction
            .execute(
                "DELETE FROM constitution_clauses WHERE tenant_id = $1 AND repository_id = $2",
                &[&request.tenant_id, &request.repository_id],
            )
            .await?;
        for clause in &request.clauses {
            transaction
                .execute(
                    "INSERT INTO constitution_clauses \
                     (tenant_id, repository_id, clause_id, slug, kind, title, statement, status, \
                      consequence, scope, rationale) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
                    &[
                        &request.tenant_id,
                        &request.repository_id,
                        &clause.id,
                        &clause.slug,
                        &clause.kind,
                        &clause.title,
                        &clause.statement,
                        &clause.status,
                        &clause.consequence,
                        &clause.scope,
                        &clause.rationale,
                    ],
                )
                .await?;
        }
        transaction.commit().await?;
        Ok(published_at)
    }

    pub async fn get_active(
        &self,
        tenant_id: &str,
        repository_id: &str,
    ) -> Result<Option<ActiveConstitution>, ConstitutionStoreError> {
        let Some(snapshot) = self
            .client
            .query_opt(
                "SELECT version_id, version, status, published_at FROM constitution_snapshots \
                 WHERE tenant_id = $1 AND repository_id = $2",
                &[&tenant_id, &repository_id],
            )
            .await?
        else {
            return Ok(None);
        };
        let clause_rows = self
            .client
            .query(
                "SELECT clause_id, slug, kind, title, statement, status, consequence, scope, \
                        rationale \
                 FROM constitution_clauses WHERE tenant_id = $1 AND repository_id = $2 \
                 ORDER BY clause_id",
                &[&tenant_id, &repository_id],
            )
            .await?;
        let clauses = clause_rows
            .into_iter()
            .map(|row| ClauseSnapshot {
                id: row.get("clause_id"),
                slug: row.get("slug"),
                kind: row.get("kind"),
                title: row.get("title"),
                statement: row.get("statement"),
                status: row.get("status"),
                consequence: row.get("consequence"),
                scope: row.get("scope"),
                rationale: row.get("rationale"),
            })
            .collect();
        Ok(Some(ActiveConstitution {
            version_id: snapshot.get("version_id"),
            version: snapshot.get("version"),
            status: snapshot.get("status"),
            clauses,
            published_at: snapshot.get("published_at"),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn unique_scope(label: &str) -> (String, String) {
        let mut bytes = [0u8; 8];
        getrandom::getrandom(&mut bytes).expect("the OS random source should be available");
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        (
            format!("tenant-{label}-{hex}"),
            format!("repo-{label}-{hex}"),
        )
    }

    pub(super) async fn store() -> Option<ConstitutionStore> {
        let database_url = std::env::var("ACKPLANE_TEST_DATABASE_URL").ok()?;
        Some(ConstitutionStore::connect(&database_url).await.unwrap())
    }

    pub(super) fn clause(id: &str) -> ClauseSnapshot {
        ClauseSnapshot {
            id: id.to_string(),
            slug: format!("slug-{id}"),
            kind: "constraint".to_string(),
            title: format!("title {id}"),
            statement: format!("statement {id}"),
            status: "active".to_string(),
            consequence: Some("block".to_string()),
            scope: None,
            rationale: Some("because".to_string()),
        }
    }

    #[tokio::test]
    async fn publishing_a_snapshot_makes_it_the_active_one() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("publish");

        store
            .publish(PublishConstitutionRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                version_id: "version-1".to_string(),
                version: 1,
                status: "active".to_string(),
                clauses: vec![clause("clause-a")],
            })
            .await
            .unwrap();

        let active = store
            .get_active(&tenant_id, &repository_id)
            .await
            .unwrap()
            .expect("a published snapshot should be active");

        assert_eq!(active.version_id, "version-1");
        assert_eq!(active.version, 1);
        assert_eq!(active.clauses.len(), 1);
        assert_eq!(active.clauses[0].id, "clause-a");
    }

    #[tokio::test]
    async fn a_repository_with_no_published_snapshot_reports_none() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("absent");

        let active = store.get_active(&tenant_id, &repository_id).await.unwrap();

        assert!(active.is_none());
    }

    /// A second publish replaces the snapshot wholesale: the old clause set
    /// must not survive alongside the new one.
    #[tokio::test]
    async fn republishing_replaces_the_prior_snapshot_and_its_clauses() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("replace");

        store
            .publish(PublishConstitutionRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                version_id: "version-1".to_string(),
                version: 1,
                status: "active".to_string(),
                clauses: vec![clause("clause-a"), clause("clause-b")],
            })
            .await
            .unwrap();
        store
            .publish(PublishConstitutionRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                version_id: "version-2".to_string(),
                version: 2,
                status: "active".to_string(),
                clauses: vec![clause("clause-c")],
            })
            .await
            .unwrap();

        let active = store
            .get_active(&tenant_id, &repository_id)
            .await
            .unwrap()
            .expect("the second publish should be active");

        assert_eq!(active.version_id, "version-2");
        assert_eq!(active.clauses.len(), 1);
        assert_eq!(active.clauses[0].id, "clause-c");
    }

    #[tokio::test]
    async fn publishing_an_empty_version_id_is_refused() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("empty-version");

        let result = store
            .publish(PublishConstitutionRequest {
                tenant_id,
                repository_id,
                version_id: String::new(),
                version: 1,
                status: "active".to_string(),
                clauses: vec![],
            })
            .await;

        assert!(matches!(
            result,
            Err(ConstitutionStoreError::EmptyVersionId)
        ));
    }
}
