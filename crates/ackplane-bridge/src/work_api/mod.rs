//! Browser-safe read resources for ADR-0120's Industrial Work namespace and
//! its Board Doctor diagnostic (decision 7): "a paged/filterable task list,
//! task detail and event history, declared scope/overlap, stalled and
//! waiting work, and Board Doctor findings". Read-only by design (decision
//! 8 defers every mutation to a future, separately reviewed command
//! contract).

use std::{sync::Arc, time::SystemTime};

use ackplane_server::{
    fleet::FleetStore,
    work_command_store::{
        WorkCommandAuthorization, WorkCommandKind,
        AUTHORIZATION_UNAVAILABLE_REASON as COMMAND_AUTHORIZATION_UNAVAILABLE_REASON,
    },
    work_command_vocabulary::WORK_COMMAND_OPERATIONS,
    work_store::{
        ClaimsOnlyWork, WorkDoctorFinding, WorkPublication, WorkStore, WorkStoreError, WorkTask,
        WorkTaskDetail, WorkTaskState,
    },
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Html,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

const WORK_PAGE: &str = include_str!("../../static/work.html");
const BOARD_DOCTOR_PAGE: &str = include_str!("../../static/board-doctor.html");
const DEFAULT_PAGE_SIZE: i64 = 20;
const MAX_PAGE_SIZE: i64 = 100;
/// A wait unanswered longer than this is worth an operator's attention.
const UNANSWERED_WAIT_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

#[derive(Clone)]
pub struct WorkApiState {
    work: Arc<WorkStore>,
    fleet: Arc<FleetStore>,
    tenant_id: Arc<str>,
}

impl WorkApiState {
    pub fn new(work: Arc<WorkStore>, fleet: Arc<FleetStore>, tenant_id: Arc<str>) -> Self {
        Self {
            work,
            fleet,
            tenant_id,
        }
    }
}

pub fn work_routes(state: WorkApiState) -> Router {
    Router::new()
        .route("/work", get(work_page))
        .route("/board-doctor", get(board_doctor_page))
        .route("/api/v1/repositories/:repository_id/work", get(work_list))
        .route(
            "/api/v1/repositories/:repository_id/work/doctor",
            get(work_doctor),
        )
        .route(
            "/api/v1/repositories/:repository_id/work/:task_id",
            get(work_task_detail),
        )
        .with_state(state)
}

async fn work_page() -> Html<&'static str> {
    Html(WORK_PAGE)
}

async fn board_doctor_page() -> Html<&'static str> {
    Html(BOARD_DOCTOR_PAGE)
}

async fn ensure_repository_visible(
    state: &WorkApiState,
    repository_id: &str,
) -> Result<(), StatusCode> {
    match state
        .fleet
        .repository(state.tenant_id.as_ref(), repository_id)
        .await
    {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(error) => {
            tracing::error!(%error, "Bridge Work repository visibility query failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

fn work_store_error(error: WorkStoreError) -> StatusCode {
    tracing::error!(%error, "Bridge Work query failed");
    StatusCode::INTERNAL_SERVER_ERROR
}

fn state_label(state: WorkTaskState) -> &'static str {
    match state {
        WorkTaskState::Open => "open",
        WorkTaskState::Claimed => "claimed",
        WorkTaskState::Waiting => "waiting",
        WorkTaskState::Paused => "paused",
        WorkTaskState::Blocked => "blocked",
        WorkTaskState::InReview => "in_review",
        WorkTaskState::Completed => "completed",
        WorkTaskState::Abandoned => "abandoned",
    }
}

fn parse_state(raw: &str) -> Option<WorkTaskState> {
    match raw {
        "open" => Some(WorkTaskState::Open),
        "claimed" => Some(WorkTaskState::Claimed),
        "waiting" => Some(WorkTaskState::Waiting),
        "paused" => Some(WorkTaskState::Paused),
        "blocked" => Some(WorkTaskState::Blocked),
        "in_review" => Some(WorkTaskState::InReview),
        "completed" => Some(WorkTaskState::Completed),
        "abandoned" => Some(WorkTaskState::Abandoned),
        _ => None,
    }
}

fn unix_seconds(timestamp: SystemTime) -> Option<u64> {
    timestamp
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

#[derive(Serialize)]
struct WorkTaskSummary {
    task_id: String,
    title: String,
    goal_id: Option<String>,
    state: &'static str,
    owner_id: Option<String>,
    owner_session_id: Option<String>,
    lease_expires_at_seconds: Option<u64>,
    declared_paths: Vec<String>,
    declared_symbols: Vec<String>,
    published_by: String,
    created_at_seconds: Option<u64>,
    updated_at_seconds: Option<u64>,
}

impl From<WorkTask> for WorkTaskSummary {
    fn from(task: WorkTask) -> Self {
        Self {
            task_id: task.task_id,
            title: task.title,
            goal_id: task.goal_id,
            state: state_label(task.state),
            owner_id: task.owner_id,
            owner_session_id: task.owner_session_id,
            lease_expires_at_seconds: task.lease_expires_at.and_then(unix_seconds),
            declared_paths: task.declared_paths,
            declared_symbols: task.declared_symbols,
            published_by: task.published_by,
            created_at_seconds: unix_seconds(task.created_at),
            updated_at_seconds: unix_seconds(task.updated_at),
        }
    }
}

mod detail;
mod doctor;
mod listing;

use detail::work_task_detail;
use doctor::work_doctor;
use listing::work_list;
