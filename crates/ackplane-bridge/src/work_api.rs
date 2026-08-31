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

const WORK_PAGE: &str = include_str!("../static/work.html");
const BOARD_DOCTOR_PAGE: &str = include_str!("../static/board-doctor.html");
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

#[derive(Deserialize)]
struct WorkListQuery {
    page: Option<i64>,
    page_size: Option<i64>,
    state: Option<String>,
}

#[derive(Serialize)]
struct WorkListResponse {
    items: Vec<WorkTaskSummary>,
    total: i64,
    page: i64,
    page_size: i64,
    publication: WorkPublicationResponse,
    commands: Vec<WorkCommandCapability>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
struct WorkCommandCapability {
    operation: &'static str,
    state: &'static str,
    reason: &'static str,
}

/// What the Work command routes will actually do with each operation, read off
/// the same authority they use.
///
/// This list used to report `authorization_unavailable` for all ten operations
/// unconditionally, and that was true when written. ADR-0142 then gave the
/// Bridge's hardened loopback profile a real verified principal, so the routes
/// began executing commands while the page rendering this list still showed
/// them all disabled — one authority described two ways, and the more alarming
/// description was the wrong one.
///
/// So it is derived rather than restated: `verified_principal` grants the
/// authority, and this reports what that grant contains. An operation the
/// principal does not allow still reports unavailable, with the same reason as
/// before, which keeps the honest answer available rather than replacing one
/// blanket claim with the opposite one.
///
/// `policy_available: false` is reported deliberately. ADR-0142 clause 5 says
/// Work commands gain no `AdministrationPolicy`-style layer, so `CreateWork`'s
/// ADR-0125 decision 8 exception — a verified policy classifying it as routine
/// — has nothing to consult. Saying so is not a caveat; it is the difference
/// between "this will execute" and "this will execute without a policy having
/// approved it", which a reader deciding whether to click needs.
fn command_capabilities(authorization: &WorkCommandAuthorization) -> Vec<WorkCommandCapability> {
    // Exhaustive rather than a catch-all: a new authorization variant must be
    // considered here, not silently reported as one of the existing answers.
    let (allowed, unavailable_reason): (&[WorkCommandKind], &'static str) = match authorization {
        WorkCommandAuthorization::Verified(principal) => (
            &principal.allowed_commands,
            COMMAND_AUTHORIZATION_UNAVAILABLE_REASON,
        ),
        WorkCommandAuthorization::LoopbackDevelopment => {
            (&[], COMMAND_AUTHORIZATION_UNAVAILABLE_REASON)
        }
        WorkCommandAuthorization::MissingPrincipal => (&[], COMMAND_MISSING_PRINCIPAL_REASON),
    };
    WORK_COMMAND_OPERATIONS
        .into_iter()
        .map(|operation| {
            let permitted = allowed
                .iter()
                .any(|kind| kind.operation_name() == operation);
            WorkCommandCapability {
                operation,
                state: if permitted {
                    "available_without_policy"
                } else {
                    "authorization_unavailable"
                },
                reason: if permitted {
                    COMMAND_AVAILABLE_WITHOUT_POLICY_REASON
                } else {
                    unavailable_reason
                },
            }
        })
        .collect()
}

/// Why an operation the loopback principal allows is still not simply
/// "available": it executes, and no policy layer reviewed it (ADR-0142
/// clause 5).
const COMMAND_AVAILABLE_WITHOUT_POLICY_REASON: &str =
    "The hardened loopback profile is a verified principal for this \
     single-tenant deployment, so this command executes. No policy layer is \
     adopted, so nothing classifies it as routine or requiring review.";

/// A request that reaches the command service with no principal at all is
/// refused rather than reported unavailable, so it gets its own reason instead
/// of borrowing the unavailable one.
const COMMAND_MISSING_PRINCIPAL_REASON: &str =
    "No verified principal accompanied this request, so the command service \
     refuses it without revealing whether the Work task exists.";

#[derive(Serialize)]
struct ClaimsOnlyWorkResponse {
    task_id: String,
    owner_id: String,
    branch: String,
    lease_expires_at_seconds: Option<u64>,
    declared_paths: Vec<String>,
    declared_symbols: Vec<String>,
}

impl From<ClaimsOnlyWork> for ClaimsOnlyWorkResponse {
    fn from(claim: ClaimsOnlyWork) -> Self {
        Self {
            task_id: claim.task_id,
            owner_id: claim.owner_id,
            branch: claim.branch,
            lease_expires_at_seconds: unix_seconds(claim.lease_expires_at),
            declared_paths: claim.declared_paths,
            declared_symbols: claim.declared_symbols,
        }
    }
}

#[derive(Serialize)]
struct WorkPublicationResponse {
    state: &'static str,
    claims_only_total: i64,
    claims_only: Vec<ClaimsOnlyWorkResponse>,
}

impl From<WorkPublication> for WorkPublicationResponse {
    fn from(publication: WorkPublication) -> Self {
        let state = if publication.has_work_tasks {
            "current"
        } else if publication.claims_only_total > 0 {
            "claims_only"
        } else {
            "not_published"
        };
        Self {
            state,
            claims_only_total: publication.claims_only_total,
            claims_only: publication
                .claims_only
                .into_iter()
                .map(ClaimsOnlyWorkResponse::from)
                .collect(),
        }
    }
}

async fn work_list(
    State(state): State<WorkApiState>,
    Path(repository_id): Path<String>,
    Query(query): Query<WorkListQuery>,
) -> Result<Json<WorkListResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let page = query.page.unwrap_or(1);
    if page < 1 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let page_size = query
        .page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    let state_filter = match query.state {
        Some(raw) => Some(parse_state(&raw).ok_or(StatusCode::BAD_REQUEST)?),
        None => None,
    };
    let publication = state
        .work
        .publication(state.tenant_id.as_ref(), &repository_id, SystemTime::now())
        .await
        .map_err(work_store_error)?;
    let result = state
        .work
        .list_tasks(
            state.tenant_id.as_ref(),
            &repository_id,
            state_filter,
            page,
            page_size,
        )
        .await
        .map_err(work_store_error)?;
    Ok(Json(WorkListResponse {
        items: result
            .items
            .into_iter()
            .map(WorkTaskSummary::from)
            .collect(),
        total: result.total,
        page,
        page_size,
        publication: WorkPublicationResponse::from(publication),
        commands: command_capabilities(&crate::work_command_api::verified_principal(
            state.tenant_id.as_ref(),
            &repository_id,
        )),
    }))
}

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
struct WorkTaskDetailResponse {
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

async fn work_task_detail(
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

/// One Board Doctor finding, flattened to a single tagged shape so every
/// finding kind serializes the same way; fields unused by a given `kind` are
/// `null`.
#[derive(Serialize)]
struct WorkDoctorFindingResponse {
    kind: &'static str,
    task_id: String,
    detail: String,
    related_task_id: Option<String>,
    title: Option<String>,
    goal_id: Option<String>,
    path: Option<String>,
    wait_id: Option<String>,
    question: Option<String>,
    owner_id: Option<String>,
    since_seconds: Option<u64>,
}

impl From<WorkDoctorFinding> for WorkDoctorFindingResponse {
    fn from(finding: WorkDoctorFinding) -> Self {
        match finding {
            WorkDoctorFinding::ClaimsOnly {
                task_id,
                owner_id,
                lease_expires_at,
            } => Self {
                kind: "claims_only",
                detail: "a live claim has no corresponding Work task".to_owned(),
                task_id,
                related_task_id: None,
                title: None,
                goal_id: None,
                path: None,
                wait_id: None,
                question: None,
                owner_id: Some(owner_id),
                since_seconds: unix_seconds(lease_expires_at),
            },
            WorkDoctorFinding::DuplicateTitleSameGoal {
                task_id,
                duplicate_of_task_id,
                title,
                goal_id,
            } => Self {
                kind: "duplicate_title_same_goal",
                detail: "shares an exact title with another open task under the same goal"
                    .to_owned(),
                task_id,
                related_task_id: Some(duplicate_of_task_id),
                title: Some(title),
                goal_id: Some(goal_id),
                path: None,
                wait_id: None,
                question: None,
                owner_id: None,
                since_seconds: None,
            },
            WorkDoctorFinding::ImpossibleStateLeaseCombination {
                task_id,
                state,
                detail,
            } => Self {
                kind: "impossible_state_lease_combination",
                detail: format!("{detail} (state={})", state_label(state)),
                task_id,
                related_task_id: None,
                title: None,
                goal_id: None,
                path: None,
                wait_id: None,
                question: None,
                owner_id: None,
                since_seconds: None,
            },
            WorkDoctorFinding::UnansweredWait {
                task_id,
                wait_id,
                question,
                asked_at,
            } => Self {
                kind: "unanswered_wait",
                detail: "a wait has stood unanswered past the staleness threshold".to_owned(),
                task_id,
                related_task_id: None,
                title: None,
                goal_id: None,
                path: None,
                wait_id: Some(wait_id),
                question: Some(question),
                owner_id: None,
                since_seconds: unix_seconds(asked_at),
            },
            WorkDoctorFinding::DeclaredScopeOverlap {
                task_id,
                overlaps_with_task_id,
                path,
            } => Self {
                kind: "declared_scope_overlap",
                detail: "declares a path another open task also declares".to_owned(),
                task_id,
                related_task_id: Some(overlaps_with_task_id),
                title: None,
                goal_id: None,
                path: Some(path),
                wait_id: None,
                question: None,
                owner_id: None,
                since_seconds: None,
            },
        }
    }
}

#[derive(Serialize)]
struct WorkDoctorResponse {
    findings: Vec<WorkDoctorFindingResponse>,
    checked_at_seconds: Option<u64>,
}

async fn work_doctor(
    State(state): State<WorkApiState>,
    Path(repository_id): Path<String>,
) -> Result<Json<WorkDoctorResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let now = SystemTime::now();
    let findings = state
        .work
        .board_doctor(
            state.tenant_id.as_ref(),
            &repository_id,
            now,
            UNANSWERED_WAIT_THRESHOLD,
        )
        .await
        .map_err(work_store_error)?;
    Ok(Json(WorkDoctorResponse {
        findings: findings
            .into_iter()
            .map(WorkDoctorFindingResponse::from)
            .collect(),
        checked_at_seconds: unix_seconds(now),
    }))
}
