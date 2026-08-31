//! Tenant-scoped delegation projection and receipt reads.

use super::{
    model::{row_to_event, row_to_projection},
    DelegationEvent, DelegationListCursor, DelegationListPage, DelegationProjection,
    DelegationStore, DelegationStoreError, EVENT_COLUMNS, PROJECTION_COLUMNS,
};

impl DelegationStore {
    pub async fn get(
        &self,
        tenant_id: &str,
        repository_id: &str,
        delegation_id: &str,
    ) -> Result<Option<DelegationProjection>, DelegationStoreError> {
        self.connection()
            .await?
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

    /// Lists the current authority projection in bounded durable-update
    /// order. Revocation remains visible rather than being erased from the
    /// read model.
    pub async fn list_page(
        &self,
        tenant_id: &str,
        repository_id: &str,
        after: Option<&DelegationListCursor>,
        page_size: i64,
    ) -> Result<DelegationListPage, DelegationStoreError> {
        let page_size = page_size.max(1);
        let after_source_event_position = after
            .map(|cursor| {
                i64::try_from(cursor.source_event_position).map_err(|_| {
                    DelegationStoreError::InvalidStoredNumber {
                        field: "source_event_position",
                    }
                })
            })
            .transpose()?;
        let after_delegation_id = after
            .map(|cursor| cursor.delegation_id.as_str())
            .unwrap_or_default();
        let rows = self
            .connection()
            .await?
            .query(
                &format!(
                    "SELECT {PROJECTION_COLUMNS} FROM delegation_projections \
                     WHERE tenant_id = $1 AND repository_id = $2 \
                       AND ($3::bigint IS NULL \
                            OR source_event_position > $3 \
                            OR (source_event_position = $3 AND delegation_id > $4)) \
                     ORDER BY source_event_position ASC, delegation_id ASC \
                     LIMIT $5"
                ),
                &[
                    &tenant_id,
                    &repository_id,
                    &after_source_event_position,
                    &after_delegation_id,
                    &page_size.saturating_add(1),
                ],
            )
            .await?;
        let has_next_page = rows.len() > page_size as usize;
        let entries = rows
            .iter()
            .take(page_size as usize)
            .map(row_to_projection)
            .collect::<Result<Vec<_>, _>>()?;
        let next_after = has_next_page
            .then(|| {
                entries.last().map(|entry| DelegationListCursor {
                    source_event_position: entry.source_event_position,
                    delegation_id: entry.delegation_id.clone(),
                })
            })
            .flatten();
        Ok(DelegationListPage {
            entries,
            next_after,
        })
    }

    pub async fn history(
        &self,
        tenant_id: &str,
        repository_id: &str,
        delegation_id: &str,
    ) -> Result<Vec<DelegationEvent>, DelegationStoreError> {
        let rows = self
            .connection()
            .await?
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
