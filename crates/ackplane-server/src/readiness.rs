//! A per-repository health rollup for the Bridge (ADR-0105 decision 6's
//! Workspace/Readiness row: "Project, repository, node, and agent
//! readiness"). Deliberately reads no new schema: it composes the same
//! enrollment, projection, claim, and signing-key state Fleet and Agents
//! already expose, so an operator sees one status per repository instead of
//! opening each repository's own detail panel to piece it together.

use std::time::SystemTime;

use thiserror::Error;

use crate::db_pool::{PgConnection, PgPool};
use crate::fleet::{classify_freshness, projected_stream_head_join, RepositoryFreshness};
use crate::signing_keys::{self, EnvelopeBinding, KeyResolution, SigningKeyError};

const ENROLLMENT_MIGRATION: &str = include_str!("../migrations/0003_enrollment.sql");
const PROJECTION_MIGRATION: &str = include_str!("../migrations/0002_projection.sql");
const LEDGER_MIGRATION: &str = include_str!("../migrations/0001_ledger.sql");
const CLAIM_MIGRATION: &str = include_str!("../migrations/0005_claim_delegation.sql");
const SIGNING_KEYS_MIGRATION: &str = include_str!("../migrations/0004_signing_keys.sql");

/// A repository's overall standing, derived rather than stored so it never
/// drifts from the state it is computed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadinessStatus {
    /// Nothing needs attention: the projection has caught up with the
    /// ledger, and every signing key is either resolved or was retired on
    /// purpose.
    Ready,
    /// Operating, but with something worth a look: a lagging projection, or
    /// a signing key that is expired, revoked, unknown, or bound to a
    /// mismatched identity.
    AttentionNeeded,
    /// The repository has never produced a projection at all -- it enrolled
    /// but has not yet been meaningfully exercised.
    NotReady,
}

/// One repository's readiness, as of the moment it was computed.
#[derive(Debug, Clone, PartialEq)]
pub struct RepositoryReadiness {
    pub repository_id: String,
    pub active_node_count: i64,
    pub freshness: RepositoryFreshness,
    pub active_claim_count: i64,
    pub soonest_lease_expires_at: Option<SystemTime>,
    pub signing_keys_resolved: i64,
    pub signing_keys_needing_attention: i64,
    pub status: ReadinessStatus,
}

/// One page of the tenant's readiness rollup. `total` is the count across
/// every page, matching the pagination contract [`crate::fleet::FleetPage`]
/// already established (ADR-0112).
#[derive(Debug, Clone, PartialEq)]
pub struct ReadinessPage {
    pub items: Vec<RepositoryReadiness>,
    pub total: i64,
}

#[derive(Debug, Error)]
pub enum ReadinessError {
    #[error("readiness database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("readiness store could not obtain a database connection: {0}")]
    PoolExhausted(#[from] deadpool_postgres::PoolError),
    #[error(transparent)]
    SigningKey(#[from] SigningKeyError),
}

/// A signing key resolution needing an operator's attention, distinct from
/// `Resolved` (fine) and `NotYetActive`/`Retired` (benign lifecycle states,
/// not a problem to surface).
fn resolution_needs_attention(resolution: &KeyResolution) -> bool {
    matches!(
        resolution,
        KeyResolution::Expired
            | KeyResolution::Revoked
            | KeyResolution::BindingMismatch
            | KeyResolution::Unknown
    )
}

/// Read-only access to a tenant's per-repository readiness.
pub struct ReadinessStore {
    pool: PgPool,
}

impl ReadinessStore {
    /// Takes a clone of the process's single pool (ADR-0143 decision 1), not
    /// a database URL: a store that resolved its own connection would be
    /// exactly the per-store demand the pool exists to bound. Connect and
    /// apply the same migrations the stores this rollup reads from already
    /// apply -- this store creates no schema of its own.
    pub async fn connect(pool: &PgPool) -> Result<Self, ReadinessError> {
        let mut connection = pool.get().await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::ENROLLMENT,
            ENROLLMENT_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::PROJECTION,
            PROJECTION_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::LEDGER,
            LEDGER_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::CLAIM_DELEGATION,
            CLAIM_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::SIGNING_KEYS,
            SIGNING_KEYS_MIGRATION,
        )
        .await?;
        Ok(Self { pool: pool.clone() })
    }

    async fn connection(&self) -> Result<PgConnection, ReadinessError> {
        Ok(self.pool.get().await?)
    }

    /// One page of readiness, one row per enrolled repository, ordered by
    /// repository id.
    pub async fn readiness(
        &self,
        tenant_id: &str,
        page: i64,
        page_size: i64,
        now: SystemTime,
    ) -> Result<ReadinessPage, ReadinessError> {
        let offset = (page - 1) * page_size;
        let connection = self.connection().await?;
        let repo_rows = connection
            .query(
                &format!(
                    "SELECT request.repository_id, count(*)::BIGINT, \
                        COALESCE(projected_head.position, 0), state.stream_position, \
                        COUNT(*) OVER()::BIGINT \
                 FROM enrollment_requests AS request \
                 INNER JOIN enrollment_receipts AS receipt \
                    ON receipt.tenant_id = request.tenant_id \
                   AND receipt.repository_id = request.repository_id \
                   AND receipt.request_id = request.request_id \
                 LEFT JOIN projection_state AS state \
                    ON state.tenant_id = request.tenant_id \
                   AND state.repository_id = request.repository_id \
                 {projected_head_join} \
                 WHERE request.tenant_id = $1 \
                 GROUP BY request.repository_id, projected_head.position, state.stream_position \
                 ORDER BY request.repository_id ASC \
                 LIMIT $2 OFFSET $3",
                    projected_head_join = projected_stream_head_join("request"),
                ),
                &[&tenant_id, &page_size, &offset],
            )
            .await?;

        let total = repo_rows
            .first()
            .map(|row| row.get::<_, i64>(4))
            .unwrap_or(0);
        let repository_ids: Vec<String> = repo_rows.iter().map(|row| row.get(0)).collect();

        let claim_rows = if repository_ids.is_empty() {
            Vec::new()
        } else {
            connection
                .query(
                    "SELECT repository_id, COUNT(*)::BIGINT, MIN(lease_expires_at) \
                     FROM delegated_claims \
                     WHERE tenant_id = $1 AND lease_expires_at >= $2 \
                       AND repository_id = ANY($3) \
                     GROUP BY repository_id",
                    &[&tenant_id, &now, &repository_ids],
                )
                .await?
        };
        let mut claims_by_repo: std::collections::HashMap<String, (i64, Option<SystemTime>)> =
            std::collections::HashMap::with_capacity(claim_rows.len());
        for row in claim_rows {
            let repository_id: String = row.get(0);
            claims_by_repo.insert(repository_id, (row.get(1), row.get(2)));
        }

        let mut items = Vec::with_capacity(repo_rows.len());
        for row in repo_rows {
            let repository_id: String = row.get(0);
            let active_node_count: i64 = row.get(1);
            let projected_stream_head: i64 = row.get(2);
            let projection_stream_position: Option<i64> = row.get(3);
            let freshness = classify_freshness(projected_stream_head, projection_stream_position);

            let (active_claim_count, soonest_lease_expires_at) = claims_by_repo
                .get(&repository_id)
                .copied()
                .unwrap_or((0, None));

            let keys = signing_keys::for_repository(&connection, tenant_id, &repository_id)
                .await
                .map_err(ReadinessError::SigningKey)?;
            let mut signing_keys_resolved = 0_i64;
            let mut signing_keys_needing_attention = 0_i64;
            for lifecycle in &keys {
                let binding = EnvelopeBinding {
                    signing_key_id: &lifecycle.record.signing_key_id,
                    tenant_id: &lifecycle.record.tenant_id,
                    repository_id: &lifecycle.record.repository_id,
                    producer_id: &lifecycle.record.node_id,
                    accepted_at: now,
                };
                let resolution = signing_keys::judge(lifecycle, &binding);
                if resolution_needs_attention(&resolution) {
                    signing_keys_needing_attention += 1;
                } else if matches!(resolution, KeyResolution::Resolved(_)) {
                    signing_keys_resolved += 1;
                }
            }

            let status = if matches!(freshness, RepositoryFreshness::NeverProjected) {
                ReadinessStatus::NotReady
            } else if matches!(freshness, RepositoryFreshness::Lagging)
                || signing_keys_needing_attention > 0
            {
                ReadinessStatus::AttentionNeeded
            } else {
                ReadinessStatus::Ready
            };

            items.push(RepositoryReadiness {
                repository_id,
                active_node_count,
                freshness,
                active_claim_count,
                soonest_lease_expires_at,
                signing_keys_resolved,
                signing_keys_needing_attention,
                status,
            });
        }

        Ok(ReadinessPage { items, total })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{
        claim_store::{ClaimLeaseRequest, ClaimStore},
        ledger::{DedupKey, EventEnvelope, LedgerStore, ProvenanceClass},
        projection::{Projector, StructuralFact, STRUCTURAL_FACT_PAYLOAD_TYPE},
        test_support::{enroll_and_activate, enroll_and_activate_in, uuid_ish},
    };

    fn structural_fact_payload(node_id: &str) -> Vec<u8> {
        serde_json::to_vec(&StructuralFact {
            node_id: node_id.to_string(),
            node_type: "artifact".to_string(),
            label: node_id.to_string(),
            edges: Vec::new(),
        })
        .expect("encode structural fact")
    }

    async fn append_record(
        database_url: &str,
        tenant_id: &str,
        repository_id: &str,
        producer_sequence: i64,
        node_id: &str,
        payload_type: &str,
        payload: Vec<u8>,
    ) {
        let mut ledger = LedgerStore::connect(database_url)
            .await
            .expect("connect ledger store");
        ledger
            .append(&EventEnvelope {
                key: DedupKey {
                    tenant_id: tenant_id.to_string(),
                    repository_id: repository_id.to_string(),
                    producer_id: format!("readiness-producer-{node_id}"),
                    producer_sequence,
                },
                payload_digest: vec![producer_sequence as u8],
                payload,
                schema_version: "v1".to_string(),
                occurred_at: SystemTime::now(),
                payload_type: payload_type.to_string(),
                previous_envelope_digest: None,
                signing_key_id: None,
                signature: None,
                provenance: ProvenanceClass::EnrolledNode,
            })
            .await
            .expect("append ledger record");
    }

    async fn append_structural_fact(
        database_url: &str,
        tenant_id: &str,
        repository_id: &str,
        producer_sequence: i64,
        node_id: &str,
    ) {
        let payload = structural_fact_payload(node_id);
        append_record(
            database_url,
            tenant_id,
            repository_id,
            producer_sequence,
            node_id,
            STRUCTURAL_FACT_PAYLOAD_TYPE,
            payload,
        )
        .await;
    }

    #[tokio::test]
    async fn readiness_is_not_ready_when_never_projected() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;

        let readiness = ReadinessStore::connect(&pool)
            .await
            .expect("connect readiness store");
        let page = readiness
            .readiness(&tenant_id, 1, 10, SystemTime::now())
            .await
            .expect("query readiness");

        assert_eq!(page.total, 1);
        assert_eq!(page.items.len(), 1);
        let item = &page.items[0];
        assert_eq!(item.repository_id, repository_id);
        assert_eq!(item.freshness, RepositoryFreshness::NeverProjected);
        assert_eq!(item.status, ReadinessStatus::NotReady);
        assert_eq!(item.active_claim_count, 0);
        assert_eq!(item.soonest_lease_expires_at, None);
        assert_eq!(item.signing_keys_resolved, 1);
        assert_eq!(item.signing_keys_needing_attention, 0);
    }

    #[tokio::test]
    async fn readiness_is_ready_once_the_projection_catches_up() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;
        append_structural_fact(&database_url, &tenant_id, &repository_id, 1, "n1").await;

        let projector = Projector::connect(&pool).await.expect("connect projector");
        projector
            .rebuild(&tenant_id, &repository_id)
            .await
            .expect("rebuild projection");

        let readiness = ReadinessStore::connect(&pool)
            .await
            .expect("connect readiness store");
        let page = readiness
            .readiness(&tenant_id, 1, 10, SystemTime::now())
            .await
            .expect("query readiness");

        assert_eq!(page.items[0].freshness, RepositoryFreshness::Fresh);
        assert_eq!(page.items[0].status, ReadinessStatus::Ready);
    }

    /// Regression: a caught-up repository reported `Lagging`/`AttentionNeeded`
    /// forever as soon as any non-`structural_fact` record landed after its
    /// last structural fact.
    ///
    /// Readiness classified freshness by comparing the projection checkpoint
    /// against `stream_heads.position` â€” the head of *every* record in the
    /// stream. A projection only consumes structural facts and checkpoints at
    /// the last one it projected, and the worker's own staleness query filters
    /// to the same payload type, so it saw nothing to rebuild. The gap could
    /// never close and nothing ever cleared the warning. Evidence, knowledge,
    /// claim, directive, and delegation records all land in this same ledger,
    /// so this was the normal operating case, not an edge case â€” a permanent
    /// false amber that teaches operators to ignore the readiness signal.
    ///
    /// The fix compares the checkpoint against the head of the stream the
    /// projection actually consumes, so Readiness, Fleet, and the worker agree
    /// on "caught up" by construction. This test is also deterministic under a
    /// live projection worker: appending a non-structural record leaves
    /// nothing for the worker to rebuild, so there is no race to lose.
    #[tokio::test]
    async fn readiness_stays_ready_when_a_later_record_is_not_a_structural_fact() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;
        append_structural_fact(&database_url, &tenant_id, &repository_id, 1, "n1").await;

        let projector = Projector::connect(&pool).await.expect("connect projector");
        projector
            .rebuild(&tenant_id, &repository_id)
            .await
            .expect("rebuild projection");

        // Something that is not a structural fact lands afterwards, so the
        // whole-ledger head now leads the projection checkpoint. No rebuild
        // can ever close that gap, so it must not read as lagging.
        append_record(
            &database_url,
            &tenant_id,
            &repository_id,
            2,
            "n1",
            "evidence_record",
            b"not a structural fact".to_vec(),
        )
        .await;

        let readiness = ReadinessStore::connect(&pool)
            .await
            .expect("connect readiness store");
        let page = readiness
            .readiness(&tenant_id, 1, 10, SystemTime::now())
            .await
            .expect("query readiness");

        assert_eq!(page.items[0].freshness, RepositoryFreshness::Fresh);
        assert_eq!(page.items[0].status, ReadinessStatus::Ready);
    }

    #[tokio::test]
    async fn readiness_needs_attention_when_the_projection_is_lagging() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;
        append_structural_fact(&database_url, &tenant_id, &repository_id, 1, "n1").await;

        let projector = Projector::connect(&pool).await.expect("connect projector");
        projector
            .rebuild(&tenant_id, &repository_id)
            .await
            .expect("rebuild projection");

        // A second fact lands after the rebuild, so the ledger now leads the
        // projection checkpoint without a further rebuild to catch it up.
        append_structural_fact(&database_url, &tenant_id, &repository_id, 2, "n2").await;

        let readiness = ReadinessStore::connect(&pool)
            .await
            .expect("connect readiness store");
        let page = readiness
            .readiness(&tenant_id, 1, 10, SystemTime::now())
            .await
            .expect("query readiness");

        assert_eq!(page.items[0].freshness, RepositoryFreshness::Lagging);
        assert_eq!(page.items[0].status, ReadinessStatus::AttentionNeeded);
    }

    #[tokio::test]
    async fn readiness_reports_active_claim_count_and_the_soonest_lease_expiry() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;

        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let claims = ClaimStore::connect(&crate::test_support::gated_test_pool())
            .await
            .expect("connect claim store");
        for (task_id, lease_secs) in [("task:soonest", 60), ("task:latest", 300)] {
            claims
                .delegate(
                    &ClaimLeaseRequest {
                        tenant_id: tenant_id.clone(),
                        repository_id: repository_id.clone(),
                        task_id: task_id.to_string(),
                        owner_id: "agent:readiness".to_string(),
                        branch: format!("work/{task_id}"),
                        lease: Duration::from_secs(lease_secs),
                        paths: Vec::new(),
                        symbols: Vec::new(),
                    },
                    now,
                )
                .await
                .expect("delegate claim");
        }

        let readiness = ReadinessStore::connect(&pool)
            .await
            .expect("connect readiness store");
        // The soonest claim reaches its expiry exactly here. Ackplane's
        // summaries must keep it active until the instant has passed, matching
        // the local claim authority's inclusive boundary.
        let at_expiry = now + Duration::from_secs(60);
        let page = readiness
            .readiness(&tenant_id, 1, 10, at_expiry)
            .await
            .expect("query readiness");

        assert_eq!(page.items[0].active_claim_count, 2);
        assert_eq!(page.items[0].soonest_lease_expires_at, Some(at_expiry));
    }

    #[tokio::test]
    async fn readiness_paginates_across_multiple_repositories_in_one_tenant() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let unique_id = uuid_ish();
        let tenant_id = format!("readiness-page-tenant-{unique_id}");
        let repository_ids = [
            format!("readiness-page-repo-{unique_id}-a"),
            format!("readiness-page-repo-{unique_id}-b"),
            format!("readiness-page-repo-{unique_id}-c"),
        ];
        for (index, repository_id) in repository_ids.iter().enumerate() {
            enroll_and_activate_in(
                &database_url,
                &tenant_id,
                repository_id,
                &format!("{unique_id}-page-{index}"),
            )
            .await;
        }

        let readiness = ReadinessStore::connect(&pool)
            .await
            .expect("connect readiness store");
        let first_page = readiness
            .readiness(&tenant_id, 1, 2, SystemTime::now())
            .await
            .expect("query first page");
        assert_eq!(first_page.total, 3);
        assert_eq!(first_page.items.len(), 2);
        assert_eq!(first_page.items[0].repository_id, repository_ids[0]);
        assert_eq!(first_page.items[1].repository_id, repository_ids[1]);

        let second_page = readiness
            .readiness(&tenant_id, 2, 2, SystemTime::now())
            .await
            .expect("query second page");
        assert_eq!(second_page.total, 3);
        assert_eq!(second_page.items.len(), 1);
        assert_eq!(second_page.items[0].repository_id, repository_ids[2]);
    }

    #[tokio::test]
    async fn readiness_is_tenant_scoped() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let unique_id = uuid_ish();
        let (tenant_id, _repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;

        let readiness = ReadinessStore::connect(&pool)
            .await
            .expect("connect readiness store");
        let other_tenant = readiness
            .readiness(&format!("{tenant_id}-other"), 1, 10, SystemTime::now())
            .await
            .expect("query other tenant");

        assert_eq!(other_tenant.total, 0);
        assert!(other_tenant.items.is_empty());
    }
}
