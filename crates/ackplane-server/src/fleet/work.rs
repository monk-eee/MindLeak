use super::*;

impl FleetStore {
    /// Active delegated work for one tenant-scoped repository, ordered by the
    /// lease that expires first. Expired claims remain in the authoritative
    /// ledger for audit but are not current work.
    pub async fn active_work(
        &self,
        tenant_id: &str,
        repository_id: &str,
        now: SystemTime,
        limit: i64,
    ) -> Result<Option<Vec<ActiveWorkItem>>, tokio_postgres::Error> {
        if self.repository(tenant_id, repository_id).await?.is_none() {
            return Ok(None);
        }
        let rows = self
            .client
            .query(
                "SELECT task_id, owner_id, branch, claim_started_at, lease_expires_at, \
                        claim_lapses, paths, symbols \
                 FROM delegated_claims \
                 WHERE tenant_id = $1 AND repository_id = $2 AND lease_expires_at > $3 \
                 ORDER BY lease_expires_at ASC, task_id ASC \
                 LIMIT $4",
                &[&tenant_id, &repository_id, &now, &limit],
            )
            .await?;

        Ok(Some(
            rows.into_iter()
                .map(|row| ActiveWorkItem {
                    task_id: row.get(0),
                    owner_id: row.get(1),
                    branch: row.get(2),
                    claim_started_at: row.get(3),
                    lease_expires_at: row.get(4),
                    claim_lapses: u64::try_from(row.get::<_, i64>(5)).unwrap_or(0),
                    paths: row.get(6),
                    symbols: row.get(7),
                })
                .collect(),
        ))
    }

    /// Every stranded (lease-expired) delegated claim for one tenant-scoped
    /// repository, most-recently-expired last -- the complement of
    /// `active_work`'s exclusion, and the list `recover` (ADR-0111) needs
    /// but nothing previously exposed. An operator could recover a claim
    /// only by already knowing its task id; this is how they find one.
    pub async fn stranded_claims(
        &self,
        tenant_id: &str,
        repository_id: &str,
        now: SystemTime,
        limit: i64,
    ) -> Result<Option<Vec<ActiveWorkItem>>, tokio_postgres::Error> {
        if self.repository(tenant_id, repository_id).await?.is_none() {
            return Ok(None);
        }
        let rows = self
            .client
            .query(
                "SELECT task_id, owner_id, branch, claim_started_at, lease_expires_at, \
                        claim_lapses, paths, symbols \
                 FROM delegated_claims \
                 WHERE tenant_id = $1 AND repository_id = $2 AND lease_expires_at <= $3 \
                 ORDER BY lease_expires_at ASC, task_id ASC \
                 LIMIT $4",
                &[&tenant_id, &repository_id, &now, &limit],
            )
            .await?;

        Ok(Some(
            rows.into_iter()
                .map(|row| ActiveWorkItem {
                    task_id: row.get(0),
                    owner_id: row.get(1),
                    branch: row.get(2),
                    claim_started_at: row.get(3),
                    lease_expires_at: row.get(4),
                    claim_lapses: u64::try_from(row.get::<_, i64>(5)).unwrap_or(0),
                    paths: row.get(6),
                    symbols: row.get(7),
                })
                .collect(),
        ))
    }

    /// One page of every live delegated claim across ALL of a tenant's
    /// repositories (ADR-0105 decision 5), filtered, sorted, and paged
    /// server-side -- the cross-repository "who is working on what, right
    /// now" view a Bridge operator needs without visiting each repository's
    /// own claims list individually. Same expired-claim exclusion rule as
    /// `active_work`: an expired claim remains in the ledger for audit but
    /// is not current work.
    pub async fn fleet_work(
        &self,
        tenant_id: &str,
        filter: FleetWorkFilter<'_>,
        sort: FleetWorkSort,
        page: i64,
        page_size: i64,
        now: SystemTime,
    ) -> Result<FleetWorkPage, tokio_postgres::Error> {
        let offset = (page - 1) * page_size;
        let query = format!(
            "SELECT repository_id, task_id, owner_id, branch, claim_started_at, \
                    lease_expires_at, claim_lapses, paths, symbols, \
                    COUNT(*) OVER()::BIGINT \
             FROM delegated_claims \
             WHERE tenant_id = $1 AND lease_expires_at > $2 \
               AND ($3::text IS NULL OR repository_id ILIKE '%' || $3 || '%' ESCAPE '\\') \
               AND ($4::text IS NULL OR owner_id ILIKE '%' || $4 || '%' ESCAPE '\\') \
             {order_by} \
             LIMIT $5 OFFSET $6",
            order_by = sort.order_by_clause(),
        );
        let rows = self
            .client
            .query(
                &query,
                &[
                    &tenant_id,
                    &now,
                    &filter.repository_id,
                    &filter.owner_id,
                    &page_size,
                    &offset,
                ],
            )
            .await?;

        let total = rows.first().map(|row| row.get::<_, i64>(9)).unwrap_or(0);
        let items = rows
            .into_iter()
            .map(|row| FleetWorkItem {
                repository_id: row.get(0),
                task_id: row.get(1),
                owner_id: row.get(2),
                branch: row.get(3),
                claim_started_at: row.get(4),
                lease_expires_at: row.get(5),
                claim_lapses: u64::try_from(row.get::<_, i64>(6)).unwrap_or(0),
                paths: row.get(7),
                symbols: row.get(8),
            })
            .collect();
        Ok(FleetWorkPage { items, total })
    }

    /// One delegated claim's current owner and scope, regardless of whether
    /// its lease has expired (unlike `active_work`, which excludes expired
    /// rows because it answers a different question - "what is current
    /// work"). This answers what a recovery decision needs: the exact owner
    /// of a claim that may already be stranded, so `ackplane-bridge` can pass
    /// it as `ClaimRecoverRequest::expected_owner` without asking the caller
    /// to supply (and possibly mis-type) it.
    pub async fn claim_owner(
        &self,
        tenant_id: &str,
        repository_id: &str,
        task_id: &str,
    ) -> Result<Option<ActiveWorkItem>, tokio_postgres::Error> {
        let row = self
            .client
            .query_opt(
                "SELECT task_id, owner_id, branch, claim_started_at, lease_expires_at, \
                        claim_lapses, paths, symbols \
                 FROM delegated_claims \
                 WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
                &[&tenant_id, &repository_id, &task_id],
            )
            .await?;

        Ok(row.map(|row| ActiveWorkItem {
            task_id: row.get(0),
            owner_id: row.get(1),
            branch: row.get(2),
            claim_started_at: row.get(3),
            lease_expires_at: row.get(4),
            claim_lapses: u64::try_from(row.get::<_, i64>(5)).unwrap_or(0),
            paths: row.get(6),
            symbols: row.get(7),
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;
    use crate::{
        claim_store::{ClaimLeaseOutcome, ClaimLeaseRequest, ClaimRecoverRequest, ClaimStore},
        test_support::{enroll_and_activate_in, uuid_ish},
    };

    #[tokio::test]
    async fn active_work_is_tenant_scoped_and_excludes_expired_claims() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            crate::test_support::enroll_and_activate(&database_url, &unique_id.to_string()).await;
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let mut claims = ClaimStore::connect(&database_url)
            .await
            .expect("connect claim store");

        for (task_id, owner_id, lease_secs) in [
            ("task:expired", "agent:expired", 30),
            ("task:active", "agent:active", 120),
        ] {
            claims
                .delegate(
                    &ClaimLeaseRequest {
                        tenant_id: tenant_id.clone(),
                        repository_id: repository_id.clone(),
                        task_id: task_id.to_string(),
                        owner_id: owner_id.to_string(),
                        branch: format!("work/{task_id}"),
                        lease: Duration::from_secs(lease_secs),
                        paths: vec![format!("src/{task_id}.rs")],
                        symbols: vec![format!("symbol:{task_id}")],
                    },
                    now,
                )
                .await
                .expect("delegate claim");
        }

        let fleet = FleetStore::connect(&database_url)
            .await
            .expect("connect fleet store");
        let active = fleet
            .active_work(
                &tenant_id,
                &repository_id,
                now + Duration::from_secs(31),
                50,
            )
            .await
            .expect("query active work");

        assert_eq!(
            active.expect("repository is enrolled"),
            vec![ActiveWorkItem {
                task_id: "task:active".to_string(),
                owner_id: "agent:active".to_string(),
                branch: "work/task:active".to_string(),
                claim_started_at: now,
                lease_expires_at: now + Duration::from_secs(120),
                claim_lapses: 0,
                paths: vec!["src/task:active.rs".to_string()],
                symbols: vec!["symbol:task:active".to_string()],
            }]
        );
        assert!(fleet
            .active_work(
                &format!("{tenant_id}-other"),
                &repository_id,
                now + Duration::from_secs(31),
                50,
            )
            .await
            .expect("query wrong tenant")
            .is_none());
    }

    #[tokio::test]
    async fn stranded_claims_is_tenant_scoped_and_excludes_active_claims() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            crate::test_support::enroll_and_activate(&database_url, &unique_id.to_string()).await;
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let mut claims = ClaimStore::connect(&database_url)
            .await
            .expect("connect claim store");

        for (task_id, owner_id, lease_secs) in [
            ("task:expired", "agent:expired", 30),
            ("task:active", "agent:active", 120),
        ] {
            claims
                .delegate(
                    &ClaimLeaseRequest {
                        tenant_id: tenant_id.clone(),
                        repository_id: repository_id.clone(),
                        task_id: task_id.to_string(),
                        owner_id: owner_id.to_string(),
                        branch: format!("work/{task_id}"),
                        lease: Duration::from_secs(lease_secs),
                        paths: vec![format!("src/{task_id}.rs")],
                        symbols: vec![format!("symbol:{task_id}")],
                    },
                    now,
                )
                .await
                .expect("delegate claim");
        }

        let fleet = FleetStore::connect(&database_url)
            .await
            .expect("connect fleet store");
        let stranded = fleet
            .stranded_claims(
                &tenant_id,
                &repository_id,
                now + Duration::from_secs(31),
                50,
            )
            .await
            .expect("query stranded claims");

        assert_eq!(
            stranded.expect("repository is enrolled"),
            vec![ActiveWorkItem {
                task_id: "task:expired".to_string(),
                owner_id: "agent:expired".to_string(),
                branch: "work/task:expired".to_string(),
                claim_started_at: now,
                lease_expires_at: now + Duration::from_secs(30),
                claim_lapses: 0,
                paths: vec!["src/task:expired.rs".to_string()],
                symbols: vec!["symbol:task:expired".to_string()],
            }]
        );
        assert!(fleet
            .stranded_claims(
                &format!("{tenant_id}-other"),
                &repository_id,
                now + Duration::from_secs(31),
                50,
            )
            .await
            .expect("query wrong tenant")
            .is_none());
    }

    // Regression: a recovered claim's freshness signal must show its true
    // history, not read like a brand-new claim -- `claim_started_at` stays
    // pinned to the ORIGINAL grant (ADR-0052: a same-owner recovery is a
    // continuation, not a new claim), while `claim_lapses` counts the real
    // lapse so an operator can tell a just-recovered claim apart from one
    // that has never had trouble.
    #[tokio::test]
    async fn active_work_reports_the_original_claim_start_and_a_real_lapse_count_after_recovery() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let (tenant_id, repository_id) =
            crate::test_support::enroll_and_activate(&database_url, &unique_id.to_string()).await;
        let granted_at = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let mut claims = ClaimStore::connect(&database_url)
            .await
            .expect("connect claim store");
        claims
            .delegate(
                &ClaimLeaseRequest {
                    tenant_id: tenant_id.clone(),
                    repository_id: repository_id.clone(),
                    task_id: "task:lapsed".to_string(),
                    owner_id: "agent:original".to_string(),
                    branch: "work/task:lapsed".to_string(),
                    lease: Duration::from_secs(30),
                    paths: vec!["src/lapsed.rs".to_string()],
                    symbols: vec!["symbol:lapsed".to_string()],
                },
                granted_at,
            )
            .await
            .expect("delegate the original claim");

        let recovered_at = granted_at + Duration::from_secs(60);
        let recovery = claims
            .recover(
                &ClaimRecoverRequest {
                    tenant_id: tenant_id.clone(),
                    repository_id: repository_id.clone(),
                    task_id: "task:lapsed".to_string(),
                    expected_owner: "agent:original".to_string(),
                    owner_id: "agent:original".to_string(),
                    reason: "resuming after a lapsed lease".to_string(),
                    branch: "work/task:lapsed".to_string(),
                    lease: Duration::from_secs(120),
                    paths: vec!["src/lapsed.rs".to_string()],
                    symbols: vec!["symbol:lapsed".to_string()],
                },
                recovered_at,
            )
            .await
            .expect("recover the lapsed claim");
        assert_eq!(
            recovery.outcome,
            ClaimLeaseOutcome::Granted,
            "recovering an expired claim as its own owner must be granted"
        );

        let fleet = FleetStore::connect(&database_url)
            .await
            .expect("connect fleet store");
        let active = fleet
            .active_work(
                &tenant_id,
                &repository_id,
                recovered_at + Duration::from_secs(1),
                50,
            )
            .await
            .expect("query active work")
            .expect("repository is enrolled");

        assert_eq!(
            active,
            vec![ActiveWorkItem {
                task_id: "task:lapsed".to_string(),
                owner_id: "agent:original".to_string(),
                branch: "work/task:lapsed".to_string(),
                claim_started_at: granted_at,
                lease_expires_at: recovered_at + Duration::from_secs(120),
                claim_lapses: 1,
                paths: vec!["src/lapsed.rs".to_string()],
                symbols: vec!["symbol:lapsed".to_string()],
            }],
            "claim_started_at must stay pinned to the original grant and claim_lapses must \
             count the real lapse, not reset as if this were a fresh claim"
        );
    }

    #[tokio::test]
    async fn fleet_work_aggregates_active_claims_across_every_repository_in_the_tenant() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let tenant_id = format!("fleet-work-tenant-{unique_id}");
        let repository_ids = [
            format!("fleet-work-repo-{unique_id}-a"),
            format!("fleet-work-repo-{unique_id}-b"),
        ];
        for (index, repository_id) in repository_ids.iter().enumerate() {
            enroll_and_activate_in(
                &database_url,
                &tenant_id,
                repository_id,
                &format!("{unique_id}-work-{index}"),
            )
            .await;
        }

        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let mut claims = ClaimStore::connect(&database_url)
            .await
            .expect("connect claim store");
        for (repository_id, task_id, owner_id, lease_secs) in [
            (&repository_ids[0], "task:expired", "agent:expired", 30),
            (&repository_ids[0], "task:repo-a", "agent:a", 120),
            (&repository_ids[1], "task:repo-b", "agent:b", 180),
        ] {
            claims
                .delegate(
                    &ClaimLeaseRequest {
                        tenant_id: tenant_id.clone(),
                        repository_id: repository_id.clone(),
                        task_id: task_id.to_string(),
                        owner_id: owner_id.to_string(),
                        branch: format!("work/{task_id}"),
                        lease: Duration::from_secs(lease_secs),
                        paths: vec![format!("src/{task_id}.rs")],
                        symbols: vec![format!("symbol:{task_id}")],
                    },
                    now,
                )
                .await
                .expect("delegate claim");
        }

        let fleet = FleetStore::connect(&database_url)
            .await
            .expect("connect fleet store");
        let page = fleet
            .fleet_work(
                &tenant_id,
                FleetWorkFilter::default(),
                FleetWorkSort::default_order(),
                1,
                50,
                now + Duration::from_secs(31),
            )
            .await
            .expect("query fleet work");

        assert_eq!(page.total, 2);
        assert_eq!(
            page.items,
            vec![
                FleetWorkItem {
                    repository_id: repository_ids[0].clone(),
                    task_id: "task:repo-a".to_string(),
                    owner_id: "agent:a".to_string(),
                    branch: "work/task:repo-a".to_string(),
                    claim_started_at: now,
                    lease_expires_at: now + Duration::from_secs(120),
                    claim_lapses: 0,
                    paths: vec!["src/task:repo-a.rs".to_string()],
                    symbols: vec!["symbol:task:repo-a".to_string()],
                },
                FleetWorkItem {
                    repository_id: repository_ids[1].clone(),
                    task_id: "task:repo-b".to_string(),
                    owner_id: "agent:b".to_string(),
                    branch: "work/task:repo-b".to_string(),
                    claim_started_at: now,
                    lease_expires_at: now + Duration::from_secs(180),
                    claim_lapses: 0,
                    paths: vec!["src/task:repo-b.rs".to_string()],
                    symbols: vec!["symbol:task:repo-b".to_string()],
                },
            ]
        );

        // A different tenant must see none of this tenant's active work.
        let other_tenant = fleet
            .fleet_work(
                &format!("{tenant_id}-other"),
                FleetWorkFilter::default(),
                FleetWorkSort::default_order(),
                1,
                50,
                now + Duration::from_secs(31),
            )
            .await
            .expect("query other tenant");
        assert_eq!(other_tenant.total, 0);
        assert!(other_tenant.items.is_empty());
    }

    #[tokio::test]
    async fn fleet_work_filters_by_repository_id_and_owner_id_substring() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let tenant_id = format!("fleet-work-filter-tenant-{unique_id}");
        let repository_ids = [
            format!("fleet-work-filter-repo-{unique_id}-a"),
            format!("fleet-work-filter-repo-{unique_id}-b"),
        ];
        for (index, repository_id) in repository_ids.iter().enumerate() {
            enroll_and_activate_in(
                &database_url,
                &tenant_id,
                repository_id,
                &format!("{unique_id}-filter-{index}"),
            )
            .await;
        }

        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let mut claims = ClaimStore::connect(&database_url)
            .await
            .expect("connect claim store");
        for (repository_id, task_id, owner_id) in [
            (&repository_ids[0], "task:alpha", "agent:carter"),
            (&repository_ids[1], "task:beta", "agent:delta"),
        ] {
            claims
                .delegate(
                    &ClaimLeaseRequest {
                        tenant_id: tenant_id.clone(),
                        repository_id: repository_id.clone(),
                        task_id: task_id.to_string(),
                        owner_id: owner_id.to_string(),
                        branch: format!("work/{task_id}"),
                        lease: Duration::from_secs(120),
                        paths: vec![],
                        symbols: vec![],
                    },
                    now,
                )
                .await
                .expect("delegate claim");
        }

        let fleet = FleetStore::connect(&database_url)
            .await
            .expect("connect fleet store");

        let by_repository = fleet
            .fleet_work(
                &tenant_id,
                FleetWorkFilter {
                    repository_id: Some(&escape_like_pattern(&format!("{unique_id}-a"))),
                    owner_id: None,
                },
                FleetWorkSort::default_order(),
                1,
                50,
                now,
            )
            .await
            .expect("query filtered by repository_id");
        assert_eq!(by_repository.total, 1);
        assert_eq!(by_repository.items[0].task_id, "task:alpha");

        let by_owner = fleet
            .fleet_work(
                &tenant_id,
                FleetWorkFilter {
                    repository_id: None,
                    owner_id: Some("delta"),
                },
                FleetWorkSort::default_order(),
                1,
                50,
                now,
            )
            .await
            .expect("query filtered by owner_id");
        assert_eq!(by_owner.total, 1);
        assert_eq!(by_owner.items[0].task_id, "task:beta");
    }

    #[tokio::test]
    async fn fleet_work_sort_direction_reverses_the_default_order() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique_id = uuid_ish();
        let tenant_id = format!("fleet-work-sort-tenant-{unique_id}");
        let repository_id = format!("fleet-work-sort-repo-{unique_id}");
        enroll_and_activate_in(
            &database_url,
            &tenant_id,
            &repository_id,
            &unique_id.to_string(),
        )
        .await;

        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let mut claims = ClaimStore::connect(&database_url)
            .await
            .expect("connect claim store");
        for (task_id, lease_secs) in [("task:soonest", 60), ("task:latest", 300)] {
            claims
                .delegate(
                    &ClaimLeaseRequest {
                        tenant_id: tenant_id.clone(),
                        repository_id: repository_id.clone(),
                        task_id: task_id.to_string(),
                        owner_id: "agent:sort".to_string(),
                        branch: format!("work/{task_id}"),
                        lease: Duration::from_secs(lease_secs),
                        paths: vec![],
                        symbols: vec![],
                    },
                    now,
                )
                .await
                .expect("delegate claim");
        }

        let fleet = FleetStore::connect(&database_url)
            .await
            .expect("connect fleet store");

        let ascending = fleet
            .fleet_work(
                &tenant_id,
                FleetWorkFilter::default(),
                FleetWorkSort::default_order(),
                1,
                50,
                now,
            )
            .await
            .expect("query ascending");
        assert_eq!(ascending.items[0].task_id, "task:soonest");
        assert_eq!(ascending.items[1].task_id, "task:latest");

        let descending = fleet
            .fleet_work(
                &tenant_id,
                FleetWorkFilter::default(),
                FleetWorkSort {
                    field: FleetWorkSortField::LeaseExpiresAt,
                    direction: SortDirection::Descending,
                },
                1,
                50,
                now,
            )
            .await
            .expect("query descending");
        assert_eq!(descending.items[0].task_id, "task:latest");
        assert_eq!(descending.items[1].task_id, "task:soonest");
    }
}
