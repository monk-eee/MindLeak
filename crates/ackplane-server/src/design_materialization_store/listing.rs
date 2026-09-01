use super::*;

impl MaterializationStore {
    pub async fn get_materialization(
        &self,
        tenant_id: &str,
        repository_id: &str,
        design_id: &str,
        revision_number: i64,
    ) -> Result<Option<MaterializationRevision>, MaterializationStoreError> {
        let Some(row) = self
            .connection()
            .await?
            .query_opt(
                "SELECT actor, idempotency_key, rationale, constitution_version_id, goal_ids, \
                        payload_digest, recorded_at, display_label \
                 FROM industrial_design_materializations \
                 WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3 \
                   AND revision_number = $4",
                &[&tenant_id, &repository_id, &design_id, &revision_number],
            )
            .await?
        else {
            return Ok(None);
        };
        let work_task_ids = self
            .work_task_ids_for(tenant_id, repository_id, design_id, revision_number)
            .await?;
        Ok(Some(MaterializationRevision {
            design_id: design_id.to_string(),
            revision_number,
            actor: row.get(0),
            idempotency_key: row.get(1),
            rationale: row.get(2),
            constitution_version_id: row.get(3),
            work_task_ids,
            goal_ids: row.get(4),
            payload_digest: row.get(5),
            recorded_at: row.get(6),
            display_label: row.get(7),
        }))
    }

    pub async fn list_materializations(
        &self,
        tenant_id: &str,
        repository_id: &str,
        design_id: &str,
    ) -> Result<Vec<MaterializationRevision>, MaterializationStoreError> {
        let rows = self
            .connection()
            .await?
            .query(
                "SELECT revision_number, actor, idempotency_key, rationale, \
                        constitution_version_id, goal_ids, payload_digest, recorded_at, \
                        display_label \
                 FROM industrial_design_materializations \
                 WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3 \
                 ORDER BY revision_number ASC",
                &[&tenant_id, &repository_id, &design_id],
            )
            .await?;
        let mut revisions = Vec::with_capacity(rows.len());
        for row in rows {
            let revision_number: i64 = row.get(0);
            let work_task_ids = self
                .work_task_ids_for(tenant_id, repository_id, design_id, revision_number)
                .await?;
            revisions.push(MaterializationRevision {
                design_id: design_id.to_string(),
                revision_number,
                actor: row.get(1),
                idempotency_key: row.get(2),
                rationale: row.get(3),
                constitution_version_id: row.get(4),
                work_task_ids,
                goal_ids: row.get(5),
                payload_digest: row.get(6),
                recorded_at: row.get(7),
                display_label: row.get(8),
            });
        }
        Ok(revisions)
    }
}
