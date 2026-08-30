use std::collections::HashSet;

use super::*;

const MAX_CLAIMS_ONLY_ROWS: i64 = 20;

#[derive(Debug, Clone, PartialEq)]
pub struct ClaimsOnlyWork {
    pub task_id: String,
    pub owner_id: String,
    pub branch: String,
    pub lease_expires_at: SystemTime,
    pub declared_paths: Vec<String>,
    pub declared_symbols: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkPublication {
    pub has_work_tasks: bool,
    pub claims_only_total: i64,
    pub claims_only: Vec<ClaimsOnlyWork>,
}

impl WorkStore {
    /// Reports whether this repository has native Industrial Work records and
    /// exposes a bounded sample of live claims that lack one.
    pub async fn publication(
        &self,
        tenant_id: &str,
        repository_id: &str,
        now: SystemTime,
    ) -> Result<WorkPublication, WorkStoreError> {
        let has_work_tasks = self
            .client
            .query_one(
                "SELECT EXISTS(\
                     SELECT 1 FROM work_tasks WHERE tenant_id = $1 AND repository_id = $2\
                 ) AS has_work_tasks",
                &[&tenant_id, &repository_id],
            )
            .await?
            .get("has_work_tasks");
        let (claims_only_total, claims_only) = self
            .claims_only_records(tenant_id, repository_id, now, MAX_CLAIMS_ONLY_ROWS)
            .await?;
        Ok(WorkPublication {
            has_work_tasks,
            claims_only_total,
            claims_only,
        })
    }

    pub(in crate::work_store) async fn claims_only_records(
        &self,
        tenant_id: &str,
        repository_id: &str,
        now: SystemTime,
        max_rows: i64,
    ) -> Result<(i64, Vec<ClaimsOnlyWork>), WorkStoreError> {
        let rows = self
            .client
            .query(
                "SELECT dc.task_id, dc.owner_id, dc.branch, dc.lease_expires_at, dc.paths, dc.symbols, \
                        COUNT(*) OVER()::BIGINT AS total_count \
                 FROM delegated_claims dc \
                 WHERE dc.tenant_id = $1 AND dc.repository_id = $2 AND dc.lease_expires_at >= $3 \
                   AND NOT EXISTS ( \
                       SELECT 1 FROM work_tasks wt \
                       WHERE wt.tenant_id = dc.tenant_id AND wt.repository_id = dc.repository_id \
                         AND wt.task_id = dc.task_id \
                   ) \
                 ORDER BY dc.task_id LIMIT $4",
                &[&tenant_id, &repository_id, &now, &max_rows],
            )
            .await?;
        let claims_only_total = rows.first().map(|row| row.get("total_count")).unwrap_or(0);
        let claims_only = rows
            .iter()
            .map(|row| ClaimsOnlyWork {
                task_id: row.get("task_id"),
                owner_id: row.get("owner_id"),
                branch: row.get("branch"),
                lease_expires_at: row.get("lease_expires_at"),
                declared_paths: row.get("paths"),
                declared_symbols: row.get("symbols"),
            })
            .collect();
        Ok((claims_only_total, claims_only))
    }

    /// The `(repository_id, task_id)` of every live claim across ALL of a
    /// tenant's repositories that has no corresponding native Work task --
    /// the cross-repository analogue of `publication`'s per-repository
    /// `claims_only`, for the Bridge Agents view (ADR-0120 decision 7 read
    /// fleet-wide instead of one repository at a time).
    pub async fn fleet_claims_only_keys(
        &self,
        tenant_id: &str,
        now: SystemTime,
    ) -> Result<HashSet<(String, String)>, WorkStoreError> {
        let rows = self
            .client
            .query(
                "SELECT dc.repository_id, dc.task_id \
                 FROM delegated_claims dc \
                 WHERE dc.tenant_id = $1 AND dc.lease_expires_at >= $2 \
                   AND NOT EXISTS ( \
                       SELECT 1 FROM work_tasks wt \
                       WHERE wt.tenant_id = dc.tenant_id AND wt.repository_id = dc.repository_id \
                         AND wt.task_id = dc.task_id \
                   )",
                &[&tenant_id, &now],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| (row.get("repository_id"), row.get("task_id")))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{
        claim_store::{ClaimLeaseRequest, ClaimStore},
        test_support::{enroll_and_activate_in, uuid_ish},
    };

    fn new_task(tenant_id: &str, repository_id: &str, task_id: &str) -> NewWorkTask {
        NewWorkTask {
            tenant_id: tenant_id.to_owned(),
            repository_id: repository_id.to_owned(),
            task_id: task_id.to_owned(),
            title: "Ship the thing".to_owned(),
            acceptance: "acceptance text".to_owned(),
            goal_id: None,
            declared_paths: Vec::new(),
            declared_symbols: Vec::new(),
            published_by: "test-actor".to_owned(),
        }
    }

    #[tokio::test]
    async fn fleet_claims_only_keys_spans_every_repository_and_excludes_a_published_claim() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let unique = uuid_ish();
        let tenant_id = format!("tenant-{unique}");
        let published_repository_id = format!("published-repo-{unique}");
        let orphan_repository_id = format!("orphan-repo-{unique}");
        let other_tenant_id = format!("other-tenant-{unique}");
        let other_repository_id = format!("other-repo-{unique}");
        enroll_and_activate_in(
            &database_url,
            &tenant_id,
            &published_repository_id,
            &format!("{unique}-published"),
        )
        .await;
        enroll_and_activate_in(
            &database_url,
            &tenant_id,
            &orphan_repository_id,
            &format!("{unique}-orphan"),
        )
        .await;
        enroll_and_activate_in(
            &database_url,
            &other_tenant_id,
            &other_repository_id,
            &format!("{unique}-other"),
        )
        .await;

        let published_task_id = format!("task-published-{unique}");
        let orphan_task_id = format!("task-orphan-{unique}");
        let other_task_id = format!("task-other-{unique}");
        let now = SystemTime::now();
        let mut work = WorkStore::connect(&database_url)
            .await
            .expect("connect work store");
        work.create_task(
            &new_task(&tenant_id, &published_repository_id, &published_task_id),
            &format!("event-{unique}"),
            now,
        )
        .await
        .expect("create the published task");

        let claims = ClaimStore::connect(&crate::test_support::gated_test_pool())
            .await
            .expect("connect claim store");
        for (repository_id, task_id) in [
            (&published_repository_id, &published_task_id),
            (&orphan_repository_id, &orphan_task_id),
        ] {
            claims
                .delegate(
                    &ClaimLeaseRequest {
                        tenant_id: tenant_id.clone(),
                        repository_id: repository_id.clone(),
                        task_id: task_id.clone(),
                        owner_id: "agent:fleet-claims-only".to_string(),
                        branch: format!("work/{task_id}"),
                        lease: Duration::from_secs(3_600),
                        paths: Vec::new(),
                        symbols: Vec::new(),
                    },
                    now,
                )
                .await
                .expect("delegate claim");
        }
        claims
            .delegate(
                &ClaimLeaseRequest {
                    tenant_id: other_tenant_id.clone(),
                    repository_id: other_repository_id.clone(),
                    task_id: other_task_id.clone(),
                    owner_id: "agent:other-tenant".to_string(),
                    branch: format!("work/{other_task_id}"),
                    lease: Duration::from_secs(3_600),
                    paths: Vec::new(),
                    symbols: Vec::new(),
                },
                now,
            )
            .await
            .expect("delegate the other tenant's claim");

        // A claim is still live at the exact expiry instant. Both the
        // repository publication and cross-repository keys must retain the
        // orphaned claim until that boundary has passed.
        let at_expiry = now + Duration::from_secs(3_600);
        let publication = work
            .publication(&tenant_id, &orphan_repository_id, at_expiry)
            .await
            .expect("query repository publication");
        assert_eq!(publication.claims_only_total, 1);
        assert_eq!(publication.claims_only[0].task_id, orphan_task_id);

        let keys = work
            .fleet_claims_only_keys(&tenant_id, at_expiry)
            .await
            .expect("query fleet claims-only keys");

        assert_eq!(
            keys,
            HashSet::from([(orphan_repository_id.clone(), orphan_task_id.clone())])
        );
    }
}
