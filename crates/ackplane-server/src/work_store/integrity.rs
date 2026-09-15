use tokio_postgres::GenericClient;

use super::{WorkStore, WorkStoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WorkIntegrityReason {
    #[error("source event position is missing")]
    MissingSourcePosition,
    #[error("task history is missing")]
    MissingHistory,
    #[error("source event position does not match the latest task event")]
    EventPositionMismatch,
    #[error("task state does not match the latest task event")]
    StateMismatch,
}

impl WorkStore {
    pub(super) async fn check_integrity(
        client: &(impl GenericClient + Sync),
        tenant_id: &str,
        repository_id: Option<&str>,
        task_id: Option<&str>,
    ) -> Result<(), WorkStoreError> {
        let issue = client
            .query_opt(
                "SELECT task.repository_id, task.task_id, task.source_event_position, latest.stream_position \
                 FROM work_tasks task \
                 LEFT JOIN LATERAL ( \
                     SELECT history.stream_position, history.to_state \
                     FROM work_task_history history \
                     WHERE history.tenant_id = task.tenant_id \
                       AND history.repository_id = task.repository_id \
                       AND history.task_id = task.task_id \
                     ORDER BY history.stream_position DESC LIMIT 1 \
                 ) latest ON TRUE \
                 WHERE task.tenant_id = $1 AND ($2::TEXT IS NULL OR task.repository_id = $2) \
                   AND ($3::TEXT IS NULL OR task.task_id = $3) \
                   AND (task.source_event_position IS NULL OR latest.stream_position IS NULL \
                        OR task.source_event_position IS DISTINCT FROM latest.stream_position \
                        OR task.state IS DISTINCT FROM latest.to_state) \
                 ORDER BY task.repository_id COLLATE \"C\", task.task_id COLLATE \"C\" LIMIT 1",
                &[&tenant_id, &repository_id, &task_id],
            )
            .await?;
        let Some(issue) = issue else {
            return Ok(());
        };
        let source: Option<i64> = issue.get("source_event_position");
        let latest: Option<i64> = issue.get("stream_position");
        let reason = match (source, latest) {
            (_, None) => WorkIntegrityReason::MissingHistory,
            (None, Some(_)) => WorkIntegrityReason::MissingSourcePosition,
            (Some(source), Some(latest)) if source != latest => {
                WorkIntegrityReason::EventPositionMismatch
            }
            (Some(_), Some(_)) => WorkIntegrityReason::StateMismatch,
        };
        Err(WorkStoreError::InconsistentProjection {
            repository_id: issue.get("repository_id"),
            task_id: issue.get("task_id"),
            reason,
        })
    }
}
