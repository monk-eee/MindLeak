//! Tenant-scoped delegation projection and receipt reads.

use super::{
    model::{row_to_event, row_to_projection},
    DelegationEvent, DelegationProjection, DelegationStore, DelegationStoreError, EVENT_COLUMNS,
    PROJECTION_COLUMNS,
};

impl DelegationStore {
    pub async fn get(
        &self,
        tenant_id: &str,
        repository_id: &str,
        delegation_id: &str,
    ) -> Result<Option<DelegationProjection>, DelegationStoreError> {
        self.client
            .query_opt(
                &format!(
                    "SELECT {PROJECTION_COLUMNS} FROM delegation_projections \
                     WHERE tenant_id = $1 AND repository_id = $2 AND delegation_id = $3"
                ),
                &[&tenant_id, &repository_id, &delegation_id],
            )
            .await?
            .map(|row| row_to_projection(&row))
            .transpose()
    }

    /// Lists the current authority projection in its durable update order.
    /// Revocation remains visible rather than being erased from the read model.
    pub async fn list(
        &self,
        tenant_id: &str,
        repository_id: &str,
    ) -> Result<Vec<DelegationProjection>, DelegationStoreError> {
        let rows = self
            .client
            .query(
                &format!(
                    "SELECT {PROJECTION_COLUMNS} FROM delegation_projections \
                     WHERE tenant_id = $1 AND repository_id = $2 \
                     ORDER BY source_event_position ASC, delegation_id ASC"
                ),
                &[&tenant_id, &repository_id],
            )
            .await?;
        rows.iter().map(row_to_projection).collect()
    }

    pub async fn history(
        &self,
        tenant_id: &str,
        repository_id: &str,
        delegation_id: &str,
    ) -> Result<Vec<DelegationEvent>, DelegationStoreError> {
        let rows = self
            .client
            .query(
                &format!(
                    "SELECT {EVENT_COLUMNS} FROM delegation_events \
                     WHERE tenant_id = $1 AND repository_id = $2 AND delegation_id = $3 \
                     ORDER BY stream_position ASC"
                ),
                &[&tenant_id, &repository_id, &delegation_id],
            )
            .await?;
        rows.iter().map(row_to_event).collect()
    }
}

#[cfg(test)]
mod tests;
