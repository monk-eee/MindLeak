//! One Work task's detail, event history, and waits.

use super::*;

#[derive(Serialize)]
struct WorkTaskEventResponse {
    event_id: String,
    from_state: Option<&'static str>,
    to_state: &'static str,
    actor_id: String,
    recorded_at_seconds: Option<u64>,
}

#[derive(Serialize)]
struct WorkTaskWaitResponse {
    wait_id: String,
    question: String,
    audience: Option<String>,
    asked_by: String,
    asked_at_seconds: Option<u64>,
    answered_by: Option<String>,
    answer: Option<String>,
    answered_at_seconds: Option<u64>,
}

#[derive(Serialize)]
pub(super) struct WorkTaskDetailResponse {
    task: WorkTaskSummary,
    acceptance: String,
    history: Vec<WorkTaskEventResponse>,
    waits: Vec<WorkTaskWaitResponse>,
}

impl From<WorkTaskDetail> for WorkTaskDetailResponse {
    fn from(detail: WorkTaskDetail) -> Self {
        Self {
            acceptance: detail.task.acceptance.clone(),
            history: detail
                .history
                .into_iter()
                .map(|event| WorkTaskEventResponse {
                    event_id: event.event_id,
                    from_state: event.from_state.map(state_label),
                    to_state: state_label(event.to_state),
                    actor_id: event.actor_id,
                    recorded_at_seconds: unix_seconds(event.recorded_at),
                })
                .collect(),
            waits: detail
                .waits
                .into_iter()
                .map(|wait| WorkTaskWaitResponse {
                    wait_id: wait.wait_id,
                    question: wait.question,
                    audience: wait.audience,
                    asked_by: wait.asked_by,
                    asked_at_seconds: unix_seconds(wait.asked_at),
                    answered_by: wait.answered_by,
                    answer: wait.answer,
                    answered_at_seconds: wait.answered_at.and_then(unix_seconds),
                })
                .collect(),
            task: WorkTaskSummary::from(detail.task),
        }
    }
}

pub(super) async fn work_task_detail(
    State(state): State<WorkApiState>,
    Path((repository_id, task_id)): Path<(String, String)>,
) -> Result<Json<WorkTaskDetailResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let detail = state
        .work
        .task_detail(state.tenant_id.as_ref(), &repository_id, &task_id)
        .await
        .map_err(work_store_error)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(WorkTaskDetailResponse::from(detail)))
}
