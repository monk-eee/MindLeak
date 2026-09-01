use super::*;

impl MaterializationStore {
    pub(super) async fn find_by_idempotency_key(
        &self,
        tenant_id: &str,
        repository_id: &str,
        design_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<MaterializationRevision>, MaterializationStoreError> {
        let Some(row) = self
            .connection()
            .await?
            .query_opt(
                "SELECT revision_number, actor, rationale, constitution_version_id, goal_ids, \
                        payload_digest, recorded_at, display_label \
                 FROM industrial_design_materializations \
                 WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3 \
                   AND idempotency_key = $4",
                &[&tenant_id, &repository_id, &design_id, &idempotency_key],
            )
            .await?
        else {
            return Ok(None);
        };
        let revision_number: i64 = row.get(0);
        let work_task_ids = self
            .work_task_ids_for(tenant_id, repository_id, design_id, revision_number)
            .await?;
        Ok(Some(MaterializationRevision {
            design_id: design_id.to_string(),
            revision_number,
            actor: row.get(1),
            idempotency_key: idempotency_key.to_string(),
            rationale: row.get(2),
            constitution_version_id: row.get(3),
            work_task_ids,
            goal_ids: row.get(4),
            payload_digest: row.get(5),
            recorded_at: row.get(6),
            display_label: row.get(7),
        }))
    }

    pub(super) async fn work_task_ids_for(
        &self,
        tenant_id: &str,
        repository_id: &str,
        design_id: &str,
        revision_number: i64,
    ) -> Result<Vec<String>, MaterializationStoreError> {
        let rows = self
            .connection()
            .await?
            .query(
                "SELECT work_task_id FROM industrial_design_materialization_work_tasks \
                 WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3 \
                   AND revision_number = $4 \
                 ORDER BY work_task_id",
                &[&tenant_id, &repository_id, &design_id, &revision_number],
            )
            .await?;
        Ok(rows.into_iter().map(|row| row.get(0)).collect())
    }
}
