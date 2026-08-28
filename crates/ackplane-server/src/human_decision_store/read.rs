//! Tenant-scoped human decision projection reads.
//!
//! ADR-0115 item 5 requires an escalation to appear in a human queue rather
//! than stay buried in agent logs, so the durable projection needs a bounded,
//! repository-scoped read surface. These reads never derive a decision: a
//! request past its expiry stays `Pending` here, and only a verified principal
//! resolving it through [`super::HumanDecisionStore::resolve`] can change that
//! (item 6: no response is not an approval).

use super::{
    model::row_to_projection, HumanDecisionProjection, HumanDecisionStatus, HumanDecisionStore,
    HumanDecisionStoreError, PROJECTION_COLUMNS,
};

/// Where a page of decision requests left off, in the same durable
/// `(source_event_position, decision_id)` order the list is served in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDecisionListCursor {
    pub source_event_position: u64,
    pub decision_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDecisionListPage {
    pub entries: Vec<HumanDecisionProjection>,
    pub next_after: Option<HumanDecisionListCursor>,
}

impl HumanDecisionStore {
    pub async fn get(
        &self,
        tenant_id: &str,
        repository_id: &str,
        decision_id: &str,
    ) -> Result<Option<HumanDecisionProjection>, HumanDecisionStoreError> {
        self.client
            .query_opt(
                &format!(
                    "SELECT {PROJECTION_COLUMNS} FROM human_decision_projections \
                     WHERE tenant_id = $1 AND repository_id = $2 AND decision_id = $3"
                ),
                &[&tenant_id, &repository_id, &decision_id],
            )
            .await?
            .map(|row| row_to_projection(&row))
            .transpose()
    }

    /// Lists decision requests in durable stream order, optionally narrowed to
    /// one status so a queue can show what is still waiting on a human. A
    /// resolved request stays listable rather than being erased from the read
    /// model (item 9: delegation is observable and reviewable).
    pub async fn list_page(
        &self,
        tenant_id: &str,
        repository_id: &str,
        status: Option<HumanDecisionStatus>,
        after: Option<&HumanDecisionListCursor>,
        page_size: i64,
    ) -> Result<HumanDecisionListPage, HumanDecisionStoreError> {
        let page_size = page_size.max(1);
        let after_source_event_position = after
            .map(|cursor| {
                i64::try_from(cursor.source_event_position).map_err(|_| {
                    HumanDecisionStoreError::InvalidStoredNumber {
                        field: "source_event_position",
                    }
                })
            })
            .transpose()?;
        let after_decision_id = after
            .map(|cursor| cursor.decision_id.as_str())
            .unwrap_or_default();
        let status_filter = status.map(HumanDecisionStatus::as_i16);
        let rows = self
            .client
            .query(
                &format!(
                    "SELECT {PROJECTION_COLUMNS} FROM human_decision_projections \
                     WHERE tenant_id = $1 AND repository_id = $2 \
                       AND ($3::smallint IS NULL OR status = $3) \
                       AND ($4::bigint IS NULL \
                            OR source_event_position > $4 \
                            OR (source_event_position = $4 AND decision_id > $5)) \
                     ORDER BY source_event_position ASC, decision_id ASC \
                     LIMIT $6"
                ),
                &[
                    &tenant_id,
                    &repository_id,
                    &status_filter,
                    &after_source_event_position,
                    &after_decision_id,
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
                entries.last().map(|entry| HumanDecisionListCursor {
                    source_event_position: entry.source_event_position,
                    decision_id: entry.decision_id.clone(),
                })
            })
            .flatten();
        Ok(HumanDecisionListPage {
            entries,
            next_after,
        })
    }
}
