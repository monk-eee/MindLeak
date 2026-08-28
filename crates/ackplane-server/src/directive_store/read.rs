//! Undelivered-directive reads for live NodeSync delivery (ADR-0116 slice 3).
//!
//! A directive is "pending" when it has been durably enqueued for a supervisor
//! session and that session has not yet returned a receipt for it. The receipt
//! table is the only thing consulted, deliberately: a `delivered_at` column
//! would record that Ackplane *sent* a frame, and ADR-0107 is explicit that a
//! dispatch is not evidence a supervisor acted. Redelivering an already-sent
//! directive is safe because `SupervisorInbox::receive` replays the original
//! receipt for a repeated `(directive_id, payload_digest)` rather than
//! re-processing it, so at-least-once delivery is the honest guarantee here.

use prost::Message;

use super::{DirectiveStore, DirectiveStoreError};
use ackplane_protocol::v1;

/// The largest number of directives one delivery pass may carry.
///
/// Bounded so a supervisor reconnecting to a long backlog receives it in
/// ordered instalments rather than one unbounded burst that could outrun the
/// connection's flow control.
pub const MAX_DELIVERY_BATCH: i64 = 32;

impl DirectiveStore {
    /// Directives this supervisor session still owes a receipt for, oldest
    /// first, so delivery follows the same sequence the issuer established.
    pub async fn pending_for_session(
        &self,
        tenant_id: &str,
        repository_id: &str,
        node_id: &str,
        agent_session_id: &str,
        limit: i64,
    ) -> Result<Vec<v1::AgentDirective>, DirectiveStoreError> {
        let limit = limit.clamp(1, MAX_DELIVERY_BATCH);
        let rows = self
            .client
            .query(
                "SELECT directive_payload FROM agent_directives d \
                 WHERE d.tenant_id = $1 AND d.repository_id = $2 \
                   AND d.node_id = $3 AND d.agent_session_id = $4 \
                   AND NOT EXISTS ( \
                       SELECT 1 FROM directive_receipts r \
                       WHERE r.tenant_id = d.tenant_id \
                         AND r.repository_id = d.repository_id \
                         AND r.directive_id = d.directive_id) \
                 ORDER BY d.directive_sequence ASC \
                 LIMIT $5",
                &[
                    &tenant_id,
                    &repository_id,
                    &node_id,
                    &agent_session_id,
                    &limit,
                ],
            )
            .await?;
        rows.iter()
            .map(|row| {
                let payload: Vec<u8> = row.get("directive_payload");
                v1::AgentDirective::decode(payload.as_slice())
                    .map_err(|_| DirectiveStoreError::UnsupportedDirective)
            })
            .collect()
    }
}
