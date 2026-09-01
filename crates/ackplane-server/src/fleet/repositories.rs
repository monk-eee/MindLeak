use super::*;

impl FleetStore {
    /// Return one page of repositories with an active enrollment receipt in
    /// one tenant, filtered, sorted, and paged server-side (ADR-0112).
    ///
    /// A missing projection state remains visible as `None`; it means the
    /// repository is enrolled but has not produced a graph projection yet.
    pub async fn repositories(
        &self,
        tenant_id: &str,
        filter: FleetFilter<'_>,
        sort: FleetSort,
        page: i64,
        page_size: i64,
    ) -> Result<FleetPage, FleetStoreError> {
        let offset = (page - 1) * page_size;
        let query = format!(
            "SELECT request.repository_id, count(*)::BIGINT, max(receipt.activated_at), \
                    COALESCE(head.position, 0), state.stream_position, state.projected_at, \
                    COUNT(*) OVER()::BIGINT, COALESCE(projected_head.position, 0) \
             FROM enrollment_requests AS request \
             INNER JOIN enrollment_receipts AS receipt \
                ON receipt.tenant_id = request.tenant_id \
               AND receipt.repository_id = request.repository_id \
               AND receipt.request_id = request.request_id \
             LEFT JOIN projection_state AS state \
                ON state.tenant_id = request.tenant_id \
               AND state.repository_id = request.repository_id \
             LEFT JOIN stream_heads AS head \
                ON head.tenant_id = request.tenant_id \
               AND head.repository_id = request.repository_id \
             {projected_head_join} \
             WHERE request.tenant_id = $1 \
               AND ($2::text IS NULL OR request.repository_id ILIKE '%' || $2 || '%' ESCAPE '\\') \
             GROUP BY request.repository_id, head.position, projected_head.position, \
                      state.stream_position, state.projected_at \
             HAVING ($3::text IS NULL \
                     OR ($3 = 'never_projected' AND state.stream_position IS NULL) \
                     OR ($3 = 'lagging' AND state.stream_position IS NOT NULL \
                         AND state.stream_position < COALESCE(projected_head.position, 0)) \
                     OR ($3 = 'fresh' AND state.stream_position IS NOT NULL \
                         AND state.stream_position >= COALESCE(projected_head.position, 0))) \
                 AND ($4::text IS NULL \
                      OR ($4 = 'active' AND count(*) > 0) \
                      OR ($4 = 'none' AND count(*) = 0)) \
             {order_by} \
             LIMIT $5 OFFSET $6",
            projected_head_join = projected_stream_head_join("request"),
            order_by = sort.order_by_clause(),
        );
        let rows = self
            .connection()
            .await?
            .query(
                &query,
                &[
                    &tenant_id,
                    &filter.q,
                    &filter.freshness,
                    &filter.coordination,
                    &page_size,
                    &offset,
                ],
            )
            .await?;

        let total = rows.first().map(|row| row.get::<_, i64>(6)).unwrap_or(0);
        let repositories = rows
            .into_iter()
            .map(|row| {
                let ledger_stream_position: i64 = row.get(3);
                let projection_stream_position: Option<i64> = row.get(4);
                let projected_stream_head: i64 = row.get(7);
                FleetRepository {
                    repository_id: row.get(0),
                    active_node_count: row.get(1),
                    last_activated_at: row.get(2),
                    ledger_stream_position,
                    freshness: classify_freshness(
                        projected_stream_head,
                        projection_stream_position,
                    ),
                    projection_stream_position,
                    projection_updated_at: row.get(5),
                }
            })
            .collect();

        Ok(FleetPage {
            repositories,
            total,
        })
    }

    /// One repository's coordination, ledger, and projection state, scoped to
    /// a tenant (ADR-0095 decision 4). `None` when the repository has no
    /// active enrollment receipt in that tenant — a caller that cannot see a
    /// repository must never distinguish "not enrolled" from "enrolled in a
    /// different tenant"; both read the same way here.
    pub async fn repository(
        &self,
        tenant_id: &str,
        repository_id: &str,
    ) -> Result<Option<RepositoryDetail>, FleetStoreError> {
        let row = self
            .connection()
            .await?
            .query_opt(
                &format!(
                    "SELECT request.repository_id, count(*)::BIGINT, max(receipt.activated_at), \
                        COALESCE(head.position, 0), \
                        state.stream_position, state.projected_at, \
                        COALESCE(projected_head.position, 0) \
                 FROM enrollment_requests AS request \
                 INNER JOIN enrollment_receipts AS receipt \
                    ON receipt.tenant_id = request.tenant_id \
                   AND receipt.repository_id = request.repository_id \
                   AND receipt.request_id = request.request_id \
                 LEFT JOIN projection_state AS state \
                    ON state.tenant_id = request.tenant_id \
                   AND state.repository_id = request.repository_id \
                 LEFT JOIN stream_heads AS head \
                    ON head.tenant_id = request.tenant_id \
                   AND head.repository_id = request.repository_id \
                 {projected_head_join} \
                 WHERE request.tenant_id = $1 AND request.repository_id = $2 \
                 GROUP BY request.repository_id, head.position, projected_head.position, \
                          state.stream_position, state.projected_at",
                    projected_head_join = projected_stream_head_join("request"),
                ),
                &[&tenant_id, &repository_id],
            )
            .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let ledger_stream_position: i64 = row.get(3);
        let projection_stream_position: Option<i64> = row.get(4);
        let projected_stream_head: i64 = row.get(6);
        let freshness = classify_freshness(projected_stream_head, projection_stream_position);

        Ok(Some(RepositoryDetail {
            repository_id: row.get(0),
            active_node_count: row.get(1),
            last_activated_at: row.get(2),
            ledger_stream_position,
            projection_stream_position,
            projection_updated_at: row.get(5),
            freshness,
        }))
    }

    /// A repository's most recent accepted ledger records, newest first
    /// (ADR-0095 decision 4's timeline resource). Only accepted records are
    /// ever persisted (ADR-0086), so this names accepted positions, never
    /// rejections.
    ///
    /// Each event's `key_status` is judged as of now via the same
    /// `signing_keys::judge` rule `FleetStore::signing_keys` uses (ADR-0084
    /// decision 12) -- one repository-wide key fetch, not one query per
    /// event, since a handful of enrolled keys typically sign every event in
    /// a bounded timeline page.
    ///
    /// `before` is keyset pagination (ADR-0112 decision 1): `None` returns
    /// the newest page; `Some(cursor)` continues strictly older than the
    /// last `stream_position` the caller already saw. Unlike `OFFSET`, a
    /// concurrent append between two page requests cannot skip or repeat a
    /// row, because the cursor names a fixed position rather than a shifting
    /// row count.
    pub async fn timeline(
        &self,
        tenant_id: &str,
        repository_id: &str,
        before: Option<i64>,
        limit: i64,
    ) -> Result<Vec<TimelineEvent>, FleetStoreError> {
        let connection = self.connection().await?;
        let rows = connection
            .query(
                "SELECT stream_position, occurred_at, payload_type, producer_id, signing_key_id \
                 FROM ledger_records \
                 WHERE tenant_id = $1 AND repository_id = $2 \
                 AND ($4::bigint IS NULL OR stream_position < $4) \
                 ORDER BY stream_position DESC \
                 LIMIT $3",
                &[&tenant_id, &repository_id, &limit, &before],
            )
            .await?;

        let keys_by_id: HashMap<String, _> =
            signing_keys::for_repository(&connection, tenant_id, repository_id)
                .await?
                .into_iter()
                .map(|lifecycle| (lifecycle.record.signing_key_id.clone(), lifecycle))
                .collect();
        let now = SystemTime::now();

        Ok(rows
            .into_iter()
            .map(|row| {
                let signing_key_id: Option<String> = row.get(4);
                let key_status = signing_key_id.as_deref().map(|signing_key_id| {
                    match keys_by_id.get(signing_key_id) {
                        Some(lifecycle) => {
                            let binding = EnvelopeBinding {
                                signing_key_id: &lifecycle.record.signing_key_id,
                                tenant_id: &lifecycle.record.tenant_id,
                                repository_id: &lifecycle.record.repository_id,
                                producer_id: &lifecycle.record.node_id,
                                accepted_at: now,
                            };
                            signing_keys::judge(lifecycle, &binding)
                        }
                        None => KeyResolution::Unknown,
                    }
                });
                TimelineEvent {
                    stream_position: row.get(0),
                    occurred_at: row.get(1),
                    payload_type: row.get(2),
                    producer_id: row.get(3),
                    signing_key_id,
                    key_status,
                }
            })
            .collect())
    }

    /// Every signing key this repository has ever enrolled, judged as of now.
    ///
    /// Reuses `signing_keys::judge` -- the same rule an accepted envelope is
    /// checked against -- with a binding built from each record's own
    /// identity, so it always matches by construction; only the lifecycle
    /// timestamps compared against `now` can move the answer. This is
    /// deliberately not a second judgment invented for a health view.
    pub async fn signing_keys(
        &self,
        tenant_id: &str,
        repository_id: &str,
    ) -> Result<Option<Vec<SigningKeyStatus>>, FleetStoreError> {
        if self.repository(tenant_id, repository_id).await?.is_none() {
            return Ok(None);
        }
        let now = SystemTime::now();
        let connection = self.connection().await?;
        let keys = signing_keys::for_repository(&connection, tenant_id, repository_id).await?;
        Ok(Some(
            keys.into_iter()
                .map(|lifecycle| {
                    let binding = EnvelopeBinding {
                        signing_key_id: &lifecycle.record.signing_key_id,
                        tenant_id: &lifecycle.record.tenant_id,
                        repository_id: &lifecycle.record.repository_id,
                        producer_id: &lifecycle.record.node_id,
                        accepted_at: now,
                    };
                    let status = signing_keys::judge(&lifecycle, &binding);
                    let record = lifecycle.record;
                    SigningKeyStatus {
                        signing_key_id: record.signing_key_id,
                        node_id: record.node_id,
                        public_key_fingerprint: record.public_key_fingerprint,
                        status,
                        expires_at: record.expires_at,
                    }
                })
                .collect(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;
    use crate::{
        ledger::{DedupKey, EventEnvelope, LedgerStore, ProvenanceClass},
        projection::{Projector, StructuralFact},
        test_support::{enroll_and_activate, enroll_and_activate_in, uuid_ish},
    };

    #[tokio::test]
    async fn an_enrolled_repository_without_a_projection_remains_visible_in_fleet() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;

        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test pool builds from the gated database url");

        let fleet = FleetStore::connect(&pool)
            .await
            .expect("connect fleet store");
        let page = fleet
            .repositories(
                &tenant_id,
                FleetFilter::default(),
                FleetSort::default_order(),
                1,
                20,
            )
            .await
            .expect("query fleet repositories");

        assert_eq!(page.repositories.len(), 1);
        assert_eq!(page.total, 1);
        let repository = &page.repositories[0];
        assert_eq!(repository.repository_id, repository_id);
        assert_eq!(repository.active_node_count, 1);
        assert_eq!(repository.projection_stream_position, None);
        assert_eq!(repository.projection_updated_at, None);
    }

    #[tokio::test]
    async fn a_repository_not_enrolled_in_the_tenant_is_invisible_to_repository_detail() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;

        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test pool builds from the gated database url");

        let fleet = FleetStore::connect(&pool)
            .await
            .expect("connect fleet store");

        // Right repository, wrong tenant: must read exactly like "not
        // enrolled", never like a cross-tenant peek.
        let wrong_tenant = fleet
            .repository(&format!("{tenant_id}-other"), &repository_id)
            .await
            .expect("query repository detail");
        assert!(wrong_tenant.is_none());

        // Right tenant, a repository id nobody ever enrolled.
        let never_enrolled = fleet
            .repository(&tenant_id, &format!("{repository_id}-other"))
            .await
            .expect("query repository detail");
        assert!(never_enrolled.is_none());
    }

    #[tokio::test]
    async fn repository_detail_reports_lagging_when_the_ledger_outruns_the_projection() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;

        let ledger_pool =
            crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
                .expect("the test pool builds from a valid database url");
        let ledger = LedgerStore::connect(&ledger_pool)
            .await
            .expect("connect ledger store");
        let envelope = EventEnvelope {
            key: DedupKey {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                producer_id: format!("fleet-producer-{unique_id}"),
                producer_sequence: 1,
            },
            payload: b"{}".to_vec(),
            payload_digest: vec![1, 2, 3],
            schema_version: "v1".to_string(),
            occurred_at: SystemTime::now(),
            payload_type: "structural_fact".to_string(),
            previous_envelope_digest: None,
            signing_key_id: None,
            signature: None,
            provenance: ProvenanceClass::EnrolledNode,
        };
        ledger.append(&envelope).await.expect("append envelope");

        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test pool builds from the gated database url");

        let fleet = FleetStore::connect(&pool)
            .await
            .expect("connect fleet store");
        let detail = fleet
            .repository(&tenant_id, &repository_id)
            .await
            .expect("query repository detail")
            .expect("repository is enrolled");

        assert_eq!(detail.ledger_stream_position, 1);
        assert_eq!(detail.projection_stream_position, None);
        assert_eq!(detail.freshness, RepositoryFreshness::NeverProjected);

        let timeline = fleet
            .timeline(&tenant_id, &repository_id, None, 50)
            .await
            .expect("query repository timeline");
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0].stream_position, 1);
        assert_eq!(timeline[0].payload_type, "structural_fact");
        assert_eq!(timeline[0].producer_id, envelope.key.producer_id);
    }

    /// Real keyset pagination against a real Postgres (ADR-0112 decision 1):
    /// three events, a page smaller than the total, and the second page
    /// continuing strictly older than the cursor rather than by row offset.
    #[tokio::test]
    async fn timeline_before_cursor_continues_strictly_older_than_the_cursor() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;

        let ledger_pool =
            crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
                .expect("the test pool builds from a valid database url");
        let ledger = LedgerStore::connect(&ledger_pool)
            .await
            .expect("connect ledger store");
        for sequence in 1..=3i64 {
            let envelope = EventEnvelope {
                key: DedupKey {
                    tenant_id: tenant_id.clone(),
                    repository_id: repository_id.clone(),
                    producer_id: format!("timeline-page-producer-{unique_id}"),
                    producer_sequence: sequence,
                },
                payload: b"{}".to_vec(),
                payload_digest: vec![1, 2, 3],
                schema_version: "v1".to_string(),
                occurred_at: SystemTime::now(),
                payload_type: "structural_fact".to_string(),
                previous_envelope_digest: None,
                signing_key_id: None,
                signature: None,
                provenance: ProvenanceClass::EnrolledNode,
            };
            ledger.append(&envelope).await.expect("append envelope");
        }

        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test pool builds from the gated database url");

        let fleet = FleetStore::connect(&pool)
            .await
            .expect("connect fleet store");

        let first_page = fleet
            .timeline(&tenant_id, &repository_id, None, 2)
            .await
            .expect("query first page");
        assert_eq!(
            first_page
                .iter()
                .map(|e| e.stream_position)
                .collect::<Vec<_>>(),
            vec![3, 2],
            "first page is the newest two events, newest first"
        );

        let cursor = first_page
            .last()
            .expect("first page has a last event")
            .stream_position;
        let second_page = fleet
            .timeline(&tenant_id, &repository_id, Some(cursor), 2)
            .await
            .expect("query second page");
        assert_eq!(
            second_page
                .iter()
                .map(|e| e.stream_position)
                .collect::<Vec<_>>(),
            vec![1],
            "second page continues strictly older than the cursor, not by row offset"
        );

        let exhausted = fleet
            .timeline(
                &tenant_id,
                &repository_id,
                Some(second_page[0].stream_position),
                2,
            )
            .await
            .expect("query past the oldest event");
        assert!(
            exhausted.is_empty(),
            "a cursor at the oldest event has nothing older left to page to"
        );
    }

    #[tokio::test]
    async fn repository_detail_reports_fresh_once_the_projection_catches_up() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;

        let ledger_pool =
            crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
                .expect("the test pool builds from a valid database url");
        let ledger = LedgerStore::connect(&ledger_pool)
            .await
            .expect("connect ledger store");
        let fact = StructuralFact {
            node_id: format!("artifact:fleet-{unique_id}.rs"),
            node_type: "artifact".to_string(),
            label: format!("fleet-{unique_id}.rs"),
            edges: Vec::new(),
        };
        let envelope = EventEnvelope {
            key: DedupKey {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                producer_id: format!("fleet-producer-{unique_id}"),
                producer_sequence: 1,
            },
            payload: serde_json::to_vec(&fact).expect("encode structural fact"),
            payload_digest: vec![4, 5, 6],
            schema_version: "v1".to_string(),
            occurred_at: SystemTime::now(),
            payload_type: "structural_fact".to_string(),
            previous_envelope_digest: None,
            signing_key_id: None,
            signature: None,
            provenance: ProvenanceClass::EnrolledNode,
        };
        ledger.append(&envelope).await.expect("append envelope");

        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let projector = Projector::connect(&pool).await.expect("connect projector");
        projector
            .rebuild(&tenant_id, &repository_id)
            .await
            .expect("rebuild projection");

        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test pool builds from the gated database url");

        let fleet = FleetStore::connect(&pool)
            .await
            .expect("connect fleet store");
        let detail = fleet
            .repository(&tenant_id, &repository_id)
            .await
            .expect("query repository detail")
            .expect("repository is enrolled");

        assert_eq!(detail.ledger_stream_position, 1);
        assert_eq!(detail.projection_stream_position, Some(1));
        assert_eq!(detail.freshness, RepositoryFreshness::Fresh);
    }

    /// Regression: a fully-projected repository read `Lagging` forever as soon
    /// as any non-`structural_fact` record landed after its last structural
    /// fact.
    ///
    /// Fleet classified freshness by comparing the projection checkpoint
    /// against `stream_heads.position`, the head of *every* record in the
    /// stream, while a projection only consumes structural facts and
    /// checkpoints at the last one it projected. `Projector::stale_projections`
    /// filters to that same payload type, so the worker saw nothing to rebuild
    /// and the gap could never close. Evidence, knowledge, claim, directive,
    /// and delegation records all share this ledger, so this was the normal
    /// operating case — including for the server-side `freshness` filter, which
    /// used the same comparison and so would have reported nearly every healthy
    /// repository as lagging.
    ///
    /// Note the two positions deliberately disagree here: `ledger_stream_position`
    /// stays the honest whole-ledger head (2) while the projection is correctly
    /// `Fresh` at 1, because record 2 is not something a projection consumes.
    #[tokio::test]
    async fn a_non_structural_record_after_a_rebuild_does_not_make_a_repository_lag() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;

        let ledger_pool =
            crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
                .expect("the test pool builds from a valid database url");
        let ledger = LedgerStore::connect(&ledger_pool)
            .await
            .expect("connect ledger store");
        let fact = StructuralFact {
            node_id: format!("artifact:fleet-{unique_id}.rs"),
            node_type: "artifact".to_string(),
            label: format!("fleet-{unique_id}.rs"),
            edges: Vec::new(),
        };
        let mut envelope = EventEnvelope {
            key: DedupKey {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                producer_id: format!("fleet-producer-{unique_id}"),
                producer_sequence: 1,
            },
            payload: serde_json::to_vec(&fact).expect("encode structural fact"),
            payload_digest: vec![4, 5, 6],
            schema_version: "v1".to_string(),
            occurred_at: SystemTime::now(),
            payload_type: "structural_fact".to_string(),
            previous_envelope_digest: None,
            signing_key_id: None,
            signature: None,
            provenance: ProvenanceClass::EnrolledNode,
        };
        ledger.append(&envelope).await.expect("append envelope");

        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let projector = Projector::connect(&pool).await.expect("connect projector");
        projector
            .rebuild(&tenant_id, &repository_id)
            .await
            .expect("rebuild projection");

        // Something the projection never consumes lands afterwards. No rebuild
        // can advance the checkpoint past it, so it must not read as lagging.
        envelope.key.producer_sequence = 2;
        envelope.payload = b"not a structural fact".to_vec();
        envelope.payload_type = "evidence_record".to_string();
        ledger
            .append(&envelope)
            .await
            .expect("append non-structural record");

        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test pool builds from the gated database url");

        let fleet = FleetStore::connect(&pool)
            .await
            .expect("connect fleet store");

        let detail = fleet
            .repository(&tenant_id, &repository_id)
            .await
            .expect("query repository detail")
            .expect("repository is enrolled");
        assert_eq!(
            detail.ledger_stream_position, 2,
            "the whole-ledger head is still reported honestly"
        );
        assert_eq!(detail.projection_stream_position, Some(1));
        assert_eq!(detail.freshness, RepositoryFreshness::Fresh);

        let listed = fleet
            .repositories(
                &tenant_id,
                FleetFilter::default(),
                FleetSort::default_order(),
                1,
                10,
            )
            .await
            .expect("query fleet repositories");
        assert_eq!(listed.repositories.len(), 1);
        assert_eq!(listed.repositories[0].freshness, RepositoryFreshness::Fresh);
        assert_eq!(listed.repositories[0].ledger_stream_position, 2);

        // The server-side filter must agree with the classification, not
        // re-derive it from the whole-ledger head.
        let lagging = fleet
            .repositories(
                &tenant_id,
                FleetFilter {
                    freshness: Some("lagging"),
                    ..Default::default()
                },
                FleetSort::default_order(),
                1,
                10,
            )
            .await
            .expect("query lagging-only");
        assert_eq!(
            lagging.total, 0,
            "a caught-up repository must not be returned by the lagging filter"
        );

        let fresh = fleet
            .repositories(
                &tenant_id,
                FleetFilter {
                    freshness: Some("fresh"),
                    ..Default::default()
                },
                FleetSort::default_order(),
                1,
                10,
            )
            .await
            .expect("query fresh-only");
        assert_eq!(fresh.total, 1);
        assert_eq!(fresh.repositories[0].repository_id, repository_id);
    }

    #[tokio::test]
    async fn signing_keys_reports_none_for_a_repository_that_is_not_enrolled() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;

        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test pool builds from the gated database url");

        let fleet = FleetStore::connect(&pool)
            .await
            .expect("connect fleet store");

        // Right repository, wrong tenant: must read exactly like "not
        // enrolled", never like a cross-tenant peek at another tenant's keys.
        assert!(fleet
            .signing_keys(&format!("{tenant_id}-other"), &repository_id)
            .await
            .expect("query wrong tenant")
            .is_none());

        // Right tenant, a repository id nobody ever enrolled.
        assert!(fleet
            .signing_keys(&tenant_id, &format!("{repository_id}-other"))
            .await
            .expect("query never-enrolled repository")
            .is_none());
    }

    /// `enroll_and_activate` already registers one key as a side effect of
    /// activation; a second key is registered directly and revoked, so the
    /// enrolled repository ends up with one resolved key and one revoked key
    /// -- proving `signing_keys` reports each key's OWN judged status rather
    /// than the whole repository's status collapsing to a single value.
    #[tokio::test]
    async fn signing_keys_judges_each_enrolled_key_independently_as_of_now() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;
        let activated_signing_key_id = format!("fleet-signing-key-{unique_id}");

        let (mut client, connection) =
            tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
                .await
                .expect("test database connects");
        tokio::spawn(async move {
            let _ = connection.await;
        });

        let revoked_signing_key_id = format!("fleet-signing-key-revoked-{unique_id}");
        let revoked_record = signing_keys::SigningKeyRecord {
            signing_key_id: revoked_signing_key_id.clone(),
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            node_id: format!("fleet-node-revoked-{unique_id}"),
            public_key: vec![9; 32],
            public_key_fingerprint: format!("ed25519:{revoked_signing_key_id}"),
            activated_at: SystemTime::now(),
            expires_at: None,
        };
        let transaction = client.transaction().await.expect("begin transaction");
        signing_keys::register(&transaction, &revoked_record)
            .await
            .expect("register second key");
        transaction.commit().await.expect("commit registration");

        let transaction = client.transaction().await.expect("begin transaction");
        signing_keys::revoke(
            &transaction,
            &signing_keys::KeyRevocation {
                signing_key_id: revoked_signing_key_id.clone(),
                reason: "fleet test revocation".to_owned(),
            },
            SystemTime::now(),
        )
        .await
        .expect("revoke second key");
        transaction.commit().await.expect("commit revocation");

        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test pool builds from the gated database url");

        let fleet = FleetStore::connect(&pool)
            .await
            .expect("connect fleet store");
        let keys = fleet
            .signing_keys(&tenant_id, &repository_id)
            .await
            .expect("query signing keys")
            .expect("repository is enrolled");

        assert_eq!(keys.len(), 2, "the activation key plus the revoked key");

        let activated = keys
            .iter()
            .find(|key| key.signing_key_id == activated_signing_key_id)
            .expect("the activation-issued key is present");
        assert!(
            matches!(activated.status, KeyResolution::Resolved(_)),
            "a freshly activated key must still be resolved: {:?}",
            activated.status
        );

        let revoked = keys
            .iter()
            .find(|key| key.signing_key_id == revoked_signing_key_id)
            .expect("the manually revoked key is present");
        assert_eq!(revoked.node_id, revoked_record.node_id);
        assert_eq!(
            revoked.public_key_fingerprint,
            revoked_record.public_key_fingerprint
        );
        assert_eq!(revoked.status, KeyResolution::Revoked);
        assert_eq!(revoked.expires_at, None);
    }

    /// A ledger record's own fields never change, but its `key_status`
    /// annotation is judged as of now (ADR-0084 decision 12): the same event
    /// reads `Resolved` while its signing key is healthy and `Revoked` once
    /// that key is later revoked, without the record itself being touched.
    #[tokio::test]
    async fn timeline_key_status_reflects_a_later_revocation_without_changing_the_record() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;
        let signing_key_id = format!("fleet-signing-key-{unique_id}");

        let ledger_pool =
            crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
                .expect("the test pool builds from a valid database url");
        let ledger = LedgerStore::connect(&ledger_pool)
            .await
            .expect("connect ledger store");
        let envelope = EventEnvelope {
            key: DedupKey {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                producer_id: format!("fleet-producer-{unique_id}"),
                producer_sequence: 1,
            },
            payload: b"{}".to_vec(),
            payload_digest: vec![1, 2, 3],
            schema_version: "v1".to_string(),
            occurred_at: SystemTime::now(),
            payload_type: "structural_fact".to_string(),
            previous_envelope_digest: None,
            signing_key_id: Some(signing_key_id.clone()),
            signature: None,
            provenance: ProvenanceClass::EnrolledNode,
        };
        ledger.append(&envelope).await.expect("append envelope");

        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test pool builds from the gated database url");

        let fleet = FleetStore::connect(&pool)
            .await
            .expect("connect fleet store");
        let before_revocation = fleet
            .timeline(&tenant_id, &repository_id, None, 50)
            .await
            .expect("query timeline before revocation");
        assert_eq!(before_revocation.len(), 1);
        assert_eq!(
            before_revocation[0].signing_key_id,
            Some(signing_key_id.clone())
        );
        assert!(
            matches!(
                before_revocation[0].key_status,
                Some(KeyResolution::Resolved(_))
            ),
            "a freshly activated key must still be resolved: {:?}",
            before_revocation[0].key_status
        );

        let (mut client, connection) =
            tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
                .await
                .expect("test database connects");
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let transaction = client.transaction().await.expect("begin transaction");
        signing_keys::revoke(
            &transaction,
            &signing_keys::KeyRevocation {
                signing_key_id: signing_key_id.clone(),
                reason: "timeline test revocation".to_owned(),
            },
            SystemTime::now(),
        )
        .await
        .expect("revoke the signing key");
        transaction.commit().await.expect("commit revocation");

        let after_revocation = fleet
            .timeline(&tenant_id, &repository_id, None, 50)
            .await
            .expect("query timeline after revocation");
        assert_eq!(after_revocation.len(), 1);
        // The underlying record is untouched: same position, same payload.
        assert_eq!(
            after_revocation[0].stream_position,
            before_revocation[0].stream_position
        );
        assert_eq!(
            after_revocation[0].payload_type,
            before_revocation[0].payload_type
        );
        assert_eq!(after_revocation[0].key_status, Some(KeyResolution::Revoked));
    }

    /// An event with no signing key at all (e.g. unverified-attribution
    /// provenance) must not be misreported as a healthy or unhealthy key --
    /// it has none, and `key_status` says so with `None` rather than
    /// `Unknown`, which is reserved for a `signing_key_id` that does not
    /// resolve to any registered key.
    #[tokio::test]
    async fn timeline_key_status_is_none_when_the_record_carries_no_signing_key() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            enroll_and_activate(&database_url, &unique_id.to_string()).await;

        let ledger_pool =
            crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
                .expect("the test pool builds from a valid database url");
        let ledger = LedgerStore::connect(&ledger_pool)
            .await
            .expect("connect ledger store");
        let envelope = EventEnvelope {
            key: DedupKey {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                producer_id: format!("fleet-producer-{unique_id}"),
                producer_sequence: 1,
            },
            payload: b"{}".to_vec(),
            payload_digest: vec![1, 2, 3],
            schema_version: "v1".to_string(),
            occurred_at: SystemTime::now(),
            payload_type: "structural_fact".to_string(),
            previous_envelope_digest: None,
            signing_key_id: None,
            signature: None,
            provenance: ProvenanceClass::UnverifiedAttribution,
        };
        ledger.append(&envelope).await.expect("append envelope");

        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test pool builds from the gated database url");

        let fleet = FleetStore::connect(&pool)
            .await
            .expect("connect fleet store");
        let timeline = fleet
            .timeline(&tenant_id, &repository_id, None, 50)
            .await
            .expect("query timeline");

        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0].signing_key_id, None);
        assert_eq!(timeline[0].key_status, None);
    }

    /// Real pagination against a real Postgres: three repositories enrolled
    /// under one shared tenant, a page smaller than the total, and the
    /// filtered total staying correct across both pages.
    #[tokio::test]
    async fn repositories_paginates_and_reports_the_true_total_across_pages() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let tenant_id = format!("fleet-page-tenant-{unique_id}");
        let repository_ids = [
            format!("fleet-page-repo-{unique_id}-a"),
            format!("fleet-page-repo-{unique_id}-b"),
            format!("fleet-page-repo-{unique_id}-c"),
        ];
        for (index, repository_id) in repository_ids.iter().enumerate() {
            enroll_and_activate_in(
                &database_url,
                &tenant_id,
                repository_id,
                &format!("{unique_id}-{index}"),
            )
            .await;
        }

        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test pool builds from the gated database url");

        let fleet = FleetStore::connect(&pool)
            .await
            .expect("connect fleet store");

        let first_page = fleet
            .repositories(
                &tenant_id,
                FleetFilter::default(),
                FleetSort::default_order(),
                1,
                2,
            )
            .await
            .expect("query first page");
        assert_eq!(first_page.repositories.len(), 2);
        assert_eq!(
            first_page.total, 3,
            "total counts every match, not just this page"
        );
        assert_eq!(first_page.repositories[0].repository_id, repository_ids[0]);
        assert_eq!(first_page.repositories[1].repository_id, repository_ids[1]);

        let second_page = fleet
            .repositories(
                &tenant_id,
                FleetFilter::default(),
                FleetSort::default_order(),
                2,
                2,
            )
            .await
            .expect("query second page");
        assert_eq!(second_page.repositories.len(), 1);
        assert_eq!(second_page.total, 3);
        assert_eq!(second_page.repositories[0].repository_id, repository_ids[2]);

        // The coordination filter is a real predicate, not client-side: every
        // enrolled-and-activated repository has one active node, so "active"
        // matches all three and "none" matches zero.
        let active_only = fleet
            .repositories(
                &tenant_id,
                FleetFilter {
                    coordination: Some("active"),
                    ..Default::default()
                },
                FleetSort::default_order(),
                1,
                10,
            )
            .await
            .expect("query active-only");
        assert_eq!(active_only.total, 3);

        let none_only = fleet
            .repositories(
                &tenant_id,
                FleetFilter {
                    coordination: Some("none"),
                    ..Default::default()
                },
                FleetSort::default_order(),
                1,
                10,
            )
            .await
            .expect("query none-only");
        assert_eq!(none_only.total, 0);
        assert!(none_only.repositories.is_empty());
    }

    /// `q` is a real substring filter with real escaping: a repository whose
    /// id contains a literal `_` is found by its exact text, and is NOT
    /// falsely matched by substituting a different character for that `_` --
    /// which unescaped `ILIKE` would treat as "match any one character".
    #[tokio::test]
    async fn repositories_q_filter_escapes_literal_underscores() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let tenant_id = format!("fleet-q-tenant-{unique_id}");
        let repository_id = format!("fleet-q-repo-{unique_id}_under_score");
        enroll_and_activate_in(
            &database_url,
            &tenant_id,
            &repository_id,
            &unique_id.to_string(),
        )
        .await;

        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test pool builds from the gated database url");

        let fleet = FleetStore::connect(&pool)
            .await
            .expect("connect fleet store");

        let exact_substring = format!("{unique_id}_under_score");
        let matched = fleet
            .repositories(
                &tenant_id,
                FleetFilter {
                    q: Some(&escape_like_pattern(&exact_substring)),
                    ..Default::default()
                },
                FleetSort::default_order(),
                1,
                10,
            )
            .await
            .expect("query with exact substring");
        assert_eq!(matched.total, 1);
        assert_eq!(matched.repositories[0].repository_id, repository_id);

        // Same string with the underscore swapped for a different character:
        // must NOT match, proving `_` was treated literally, not as a wildcard.
        let wrong_char_substring = format!("{unique_id}Xunder_score");
        let unmatched = fleet
            .repositories(
                &tenant_id,
                FleetFilter {
                    q: Some(&escape_like_pattern(&wrong_char_substring)),
                    ..Default::default()
                },
                FleetSort::default_order(),
                1,
                10,
            )
            .await
            .expect("query with a mismatched substring");
        assert_eq!(unmatched.total, 0);
        assert!(unmatched.repositories.is_empty());
    }

    /// Sorting is a real `ORDER BY`, not a client-side illusion: descending
    /// by repository id reverses the default ascending order.
    #[tokio::test]
    async fn repositories_sort_direction_reverses_the_default_order() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let tenant_id = format!("fleet-sort-tenant-{unique_id}");
        let repository_ids = [
            format!("fleet-sort-repo-{unique_id}-a"),
            format!("fleet-sort-repo-{unique_id}-z"),
        ];
        for (index, repository_id) in repository_ids.iter().enumerate() {
            enroll_and_activate_in(
                &database_url,
                &tenant_id,
                repository_id,
                &format!("{unique_id}-sort-{index}"),
            )
            .await;
        }

        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test pool builds from the gated database url");

        let fleet = FleetStore::connect(&pool)
            .await
            .expect("connect fleet store");

        let ascending = fleet
            .repositories(
                &tenant_id,
                FleetFilter::default(),
                FleetSort::default_order(),
                1,
                10,
            )
            .await
            .expect("query ascending");
        assert_eq!(ascending.repositories[0].repository_id, repository_ids[0]);
        assert_eq!(ascending.repositories[1].repository_id, repository_ids[1]);

        let descending = fleet
            .repositories(
                &tenant_id,
                FleetFilter::default(),
                FleetSort {
                    field: FleetSortField::RepositoryId,
                    direction: SortDirection::Descending,
                },
                1,
                10,
            )
            .await
            .expect("query descending");
        assert_eq!(descending.repositories[0].repository_id, repository_ids[1]);
        assert_eq!(descending.repositories[1].repository_id, repository_ids[0]);
    }
}
