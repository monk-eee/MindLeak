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
                 WHERE dc.tenant_id = $1 AND dc.repository_id = $2 AND dc.lease_expires_at > $3 \
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
}
