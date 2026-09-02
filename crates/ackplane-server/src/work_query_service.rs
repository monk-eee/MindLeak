//! gRPC transport for ADR-0120's read-only Industrial Work projection
//! (ADR-0139 clause 2). Every method here is a translation to `WorkStore`'s
//! already-existing read methods -- `list_tasks`, `task_detail`,
//! `board_doctor`, `publication` -- exactly as Bridge's first Work read
//! surface (`ackplane-bridge/src/work_api.rs`) already exposes them over
//! HTTP. No new authority or storage logic is added here.

use std::sync::Arc;
use std::time::SystemTime;

use ackplane_protocol::v1;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

use crate::work_store::{
    ClaimsOnlyWork, WorkDoctorFinding, WorkPublication, WorkStore, WorkStoreError, WorkTask,
    WorkTaskDetail, WorkTaskState,
};

/// A wait unanswered longer than this is worth naming in Board Doctor's
/// findings -- the same threshold Bridge's own Board Doctor page already uses.
const UNANSWERED_WAIT_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(24 * 3600);
const DEFAULT_PAGE_SIZE: i64 = 20;
const MAX_PAGE_SIZE: i64 = 100;

pub struct WorkQueryService {
    store: Arc<Mutex<WorkStore>>,
}

impl WorkQueryService {
    pub fn new(store: WorkStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }
}

#[tonic::async_trait]
impl v1::work_query_service_server::WorkQueryService for WorkQueryService {
    async fn list_work_tasks(
        &self,
        request: Request<v1::ListWorkTasksRequest>,
    ) -> Result<Response<v1::ListWorkTasksResult>, Status> {
        let request = request.into_inner();
        let tenant_id =
            required(request.tenant_id, "tenant_id").map_err(Status::invalid_argument)?;
        let repository_id =
            required(request.repository_id, "repository_id").map_err(Status::invalid_argument)?;
        let state = optional_state(&request.state)?;
        let page = if request.page <= 0 { 1 } else { request.page };
        let page_size = if request.page_size <= 0 {
            DEFAULT_PAGE_SIZE
        } else {
            request.page_size.min(MAX_PAGE_SIZE)
        };

        let store = self.store.lock().await;
        let now = SystemTime::now();
        let publication = store
            .publication(&tenant_id, &repository_id, now)
            .await
            .map_err(map_store_error)?;
        let result = store
            .list_tasks(&tenant_id, &repository_id, state, page, page_size)
            .await
            .map_err(map_store_error)?;
        drop(store);

        Ok(Response::new(v1::ListWorkTasksResult {
            items: result
                .items
                .into_iter()
                .map(task_to_wire)
                .collect::<Result<Vec<_>, String>>()
                .map_err(Status::internal)?,
            total: result.total,
            page,
            page_size,
            publication: Some(publication_to_wire(publication).map_err(Status::internal)?),
        }))
    }

    async fn get_work_task_detail(
        &self,
        request: Request<v1::WorkTaskDetailRequest>,
    ) -> Result<Response<v1::WorkTaskDetailResult>, Status> {
        let request = request.into_inner();
        let tenant_id =
            required(request.tenant_id, "tenant_id").map_err(Status::invalid_argument)?;
        let repository_id =
            required(request.repository_id, "repository_id").map_err(Status::invalid_argument)?;
        let task_id = required(request.task_id, "task_id").map_err(Status::invalid_argument)?;

        let detail = self
            .store
            .lock()
            .await
            .task_detail(&tenant_id, &repository_id, &task_id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| Status::not_found(format!("no Work task named {task_id}")))?;
        Ok(Response::new(
            detail_to_wire(detail).map_err(Status::internal)?,
        ))
    }

    async fn get_work_board_doctor(
        &self,
        request: Request<v1::WorkBoardDoctorRequest>,
    ) -> Result<Response<v1::WorkBoardDoctorResult>, Status> {
        let request = request.into_inner();
        let tenant_id =
            required(request.tenant_id, "tenant_id").map_err(Status::invalid_argument)?;
        let repository_id =
            required(request.repository_id, "repository_id").map_err(Status::invalid_argument)?;

        let findings = self
            .store
            .lock()
            .await
            .board_doctor(
                &tenant_id,
                &repository_id,
                SystemTime::now(),
                UNANSWERED_WAIT_THRESHOLD,
            )
            .await
            .map_err(map_store_error)?;
        Ok(Response::new(v1::WorkBoardDoctorResult {
            findings: findings
                .into_iter()
                .map(finding_to_wire)
                .collect::<Result<Vec<_>, String>>()
                .map_err(Status::internal)?,
        }))
    }
}

fn required(value: String, field: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        Err(format!("{field} is required"))
    } else {
        Ok(value)
    }
}

/// An unrecognised `state` filter is refused by name rather than silently
/// matching nothing -- the same discipline Bridge's own `parse_state` uses.
fn optional_state(raw: &str) -> Result<Option<WorkTaskState>, Status> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    parse_state(raw)
        .map(Some)
        .ok_or_else(|| Status::invalid_argument(format!("unrecognised work task state: {raw}")))
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

fn rfc3339(timestamp: SystemTime) -> Result<String, String> {
    crate::wire_format::rfc3339(timestamp)
        .map_err(|error| format!("could not format a Work timestamp: {error}"))
}

fn optional_rfc3339(timestamp: Option<SystemTime>) -> Result<String, String> {
    timestamp
        .map(rfc3339)
        .transpose()
        .map(Option::unwrap_or_default)
}

fn task_to_wire(task: WorkTask) -> Result<v1::WorkTaskSummary, String> {
    Ok(v1::WorkTaskSummary {
        task_id: task.task_id,
        title: task.title,
        goal_id: task.goal_id.unwrap_or_default(),
        state: state_label(task.state).to_string(),
        owner_id: task.owner_id.unwrap_or_default(),
        owner_session_id: task.owner_session_id.unwrap_or_default(),
        lease_expires_at: optional_rfc3339(task.lease_expires_at)?,
        declared_paths: task.declared_paths,
        declared_symbols: task.declared_symbols,
        published_by: task.published_by,
        created_at: rfc3339(task.created_at)?,
        updated_at: rfc3339(task.updated_at)?,
    })
}

fn claims_only_to_wire(claim: ClaimsOnlyWork) -> Result<v1::WorkClaimsOnlySummary, String> {
    Ok(v1::WorkClaimsOnlySummary {
        task_id: claim.task_id,
        owner_id: claim.owner_id,
        branch: claim.branch,
        lease_expires_at: rfc3339(claim.lease_expires_at)?,
        paths: claim.declared_paths,
        symbols: claim.declared_symbols,
    })
}

/// Mirrors Bridge's own `WorkPublicationResponse::from` mapping exactly
/// (`current`/`claims_only`/`not_published`). `lagging` and `unavailable`
/// (ADR-0120 decision 6's remaining two states) are not yet computed by
/// either read surface -- see
/// `gaps.d/work-publication-state-never-reports-lagging-or-unavailable.md`.
fn publication_to_wire(publication: WorkPublication) -> Result<v1::WorkPublicationSummary, String> {
    let state = if publication.has_work_tasks {
        "current"
    } else if publication.claims_only_total > 0 {
        "claims_only"
    } else {
        "not_published"
    };
    Ok(v1::WorkPublicationSummary {
        state: state.to_string(),
        claims_only_total: publication.claims_only_total,
        claims_only: publication
            .claims_only
            .into_iter()
            .map(claims_only_to_wire)
            .collect::<Result<Vec<_>, String>>()?,
    })
}

fn detail_to_wire(detail: WorkTaskDetail) -> Result<v1::WorkTaskDetailResult, String> {
    let history = detail
        .history
        .into_iter()
        .map(|event| {
            Ok(v1::WorkTaskEventSummary {
                event_id: event.event_id,
                from_state: event
                    .from_state
                    .map(state_label)
                    .unwrap_or_default()
                    .to_string(),
                to_state: state_label(event.to_state).to_string(),
                actor_id: event.actor_id,
                recorded_at: rfc3339(event.recorded_at)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let waits = detail
        .waits
        .into_iter()
        .map(|wait| {
            Ok(v1::WorkTaskWaitSummary {
                wait_id: wait.wait_id,
                question: wait.question,
                audience: wait.audience.unwrap_or_default(),
                asked_by: wait.asked_by,
                asked_at: rfc3339(wait.asked_at)?,
                answered_by: wait.answered_by.unwrap_or_default(),
                answer: wait.answer.unwrap_or_default(),
                answered_at: optional_rfc3339(wait.answered_at)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(v1::WorkTaskDetailResult {
        acceptance: detail.task.acceptance.clone(),
        task: Some(task_to_wire(detail.task)?),
        history,
        waits,
    })
}

fn finding_to_wire(finding: WorkDoctorFinding) -> Result<v1::WorkDoctorFindingSummary, String> {
    Ok(match finding {
        WorkDoctorFinding::ClaimsOnly {
            task_id,
            owner_id,
            lease_expires_at,
        } => v1::WorkDoctorFindingSummary {
            kind: "claims_only".to_string(),
            detail: "a live claim has no corresponding Work task".to_string(),
            task_id,
            owner_id,
            since: rfc3339(lease_expires_at)?,
            ..Default::default()
        },
        WorkDoctorFinding::DuplicateTitleSameGoal {
            task_id,
            duplicate_of_task_id,
            title,
            goal_id,
        } => v1::WorkDoctorFindingSummary {
            kind: "duplicate_title_same_goal".to_string(),
            detail: "two non-terminal tasks under the same goal share an exact title".to_string(),
            task_id,
            related_task_id: duplicate_of_task_id,
            title,
            goal_id,
            ..Default::default()
        },
        WorkDoctorFinding::ImpossibleStateLeaseCombination {
            task_id,
            state,
            detail,
        } => v1::WorkDoctorFindingSummary {
            kind: "impossible_state_lease_combination".to_string(),
            task_id,
            detail,
            related_task_id: state_label(state).to_string(),
            ..Default::default()
        },
        WorkDoctorFinding::UnansweredWait {
            task_id,
            wait_id,
            question,
            asked_at,
        } => v1::WorkDoctorFindingSummary {
            kind: "unanswered_wait".to_string(),
            detail: "a wait has stood unanswered longer than the staleness threshold".to_string(),
            task_id,
            wait_id,
            question,
            since: rfc3339(asked_at)?,
            ..Default::default()
        },
        WorkDoctorFinding::DeclaredScopeOverlap {
            task_id,
            overlaps_with_task_id,
            path,
        } => v1::WorkDoctorFindingSummary {
            kind: "declared_scope_overlap".to_string(),
            detail: "two non-terminal tasks declare an overlapping path".to_string(),
            task_id,
            related_task_id: overlaps_with_task_id,
            path,
            ..Default::default()
        },
    })
}

fn map_store_error(error: WorkStoreError) -> Status {
    match error {
        WorkStoreError::UnknownState { .. } => Status::internal(error.to_string()),
        WorkStoreError::TaskConflict { .. } => Status::invalid_argument(error.to_string()),
        WorkStoreError::Database(_) => Status::internal(error.to_string()),
        // `unavailable`, not `internal`: a saturated pool is a condition the
        // caller can retry, matching how ClaimStore reports the same failure.
        WorkStoreError::PoolExhausted(_) => Status::unavailable(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use ackplane_protocol::v1::work_query_service_server::WorkQueryService as _;

    use super::*;
    use crate::work_store::NewWorkTask;

    async fn store() -> Option<WorkStore> {
        let pool = crate::test_support::test_pool()?;
        Some(WorkStore::connect(&pool).await.expect("connect work store"))
    }

    #[tokio::test]
    async fn list_work_tasks_reports_the_page_it_created_and_its_publication_state() {
        let Some(backing) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let unique = crate::test_support::unique_id("work-query-list");
        let tenant_id = format!("tenant-{unique}");
        let repository_id = format!("repo-{unique}");
        backing
            .create_task(
                &NewWorkTask {
                    tenant_id: tenant_id.clone(),
                    repository_id: repository_id.clone(),
                    task_id: format!("task-{unique}"),
                    title: "a task this test created".to_string(),
                    acceptance: "it exists".to_string(),
                    goal_id: None,
                    declared_paths: vec![],
                    declared_symbols: vec![],
                    published_by: "test-publisher".to_string(),
                },
                &format!("event-{unique}"),
                SystemTime::now(),
            )
            .await
            .expect("create task");

        let service = WorkQueryService::new(backing);
        let response = service
            .list_work_tasks(Request::new(v1::ListWorkTasksRequest {
                tenant_id,
                repository_id,
                state: String::new(),
                page: 0,
                page_size: 0,
            }))
            .await
            .expect("list work tasks")
            .into_inner();

        assert_eq!(response.total, 1);
        assert_eq!(response.page, 1);
        assert_eq!(response.page_size, DEFAULT_PAGE_SIZE);
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].title, "a task this test created");
        let publication = response.publication.expect("publication is reported");
        assert_eq!(publication.state, "current");
    }

    #[tokio::test]
    async fn list_work_tasks_without_a_repository_scope_is_refused_rather_than_matching_everything()
    {
        let Some(backing) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let service = WorkQueryService::new(backing);
        let error = service
            .list_work_tasks(Request::new(v1::ListWorkTasksRequest {
                tenant_id: "a-tenant".to_string(),
                repository_id: String::new(),
                state: String::new(),
                page: 1,
                page_size: 20,
            }))
            .await
            .expect_err("a blank repository_id must be refused");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
        assert!(error.message().contains("repository_id"), "{error}");
    }

    #[tokio::test]
    async fn an_unrecognised_state_filter_is_refused_by_name() {
        let Some(backing) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let service = WorkQueryService::new(backing);
        let error = service
            .list_work_tasks(Request::new(v1::ListWorkTasksRequest {
                tenant_id: "a-tenant".to_string(),
                repository_id: "a-repository".to_string(),
                state: "not-a-real-state".to_string(),
                page: 1,
                page_size: 20,
            }))
            .await
            .expect_err("an unknown state must be refused");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
        assert!(error.message().contains("not-a-real-state"), "{error}");
    }

    #[tokio::test]
    async fn get_work_task_detail_reports_history_and_waits_for_the_task_it_names() {
        let Some(backing) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let unique = crate::test_support::unique_id("work-query-detail");
        let tenant_id = format!("tenant-{unique}");
        let repository_id = format!("repo-{unique}");
        let task_id = format!("task-{unique}");
        backing
            .create_task(
                &NewWorkTask {
                    tenant_id: tenant_id.clone(),
                    repository_id: repository_id.clone(),
                    task_id: task_id.clone(),
                    title: "a task with detail".to_string(),
                    acceptance: "detail is readable".to_string(),
                    goal_id: None,
                    declared_paths: vec![],
                    declared_symbols: vec![],
                    published_by: "test-publisher".to_string(),
                },
                &format!("event-{unique}"),
                SystemTime::now(),
            )
            .await
            .expect("create task");

        let service = WorkQueryService::new(backing);
        let response = service
            .get_work_task_detail(Request::new(v1::WorkTaskDetailRequest {
                tenant_id,
                repository_id,
                task_id: task_id.clone(),
            }))
            .await
            .expect("get task detail")
            .into_inner();

        assert_eq!(response.acceptance, "detail is readable");
        let task = response.task.expect("task is reported");
        assert_eq!(task.task_id, task_id);
        assert_eq!(task.state, "open");
        assert_eq!(response.history.len(), 1);
        assert_eq!(response.history[0].to_state, "open");
        assert_eq!(response.history[0].from_state, "");
        assert!(response.waits.is_empty());
    }

    #[tokio::test]
    async fn get_work_task_detail_for_an_unknown_task_reports_not_found() {
        let Some(backing) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let unique = crate::test_support::unique_id("work-query-missing");
        let service = WorkQueryService::new(backing);
        let error = service
            .get_work_task_detail(Request::new(v1::WorkTaskDetailRequest {
                tenant_id: format!("tenant-{unique}"),
                repository_id: format!("repo-{unique}"),
                task_id: format!("task-{unique}"),
            }))
            .await
            .expect_err("an unknown task must report not-found");
        assert_eq!(error.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn get_work_board_doctor_names_two_tasks_declaring_an_overlapping_path() {
        let Some(backing) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let unique = crate::test_support::unique_id("work-query-doctor");
        let tenant_id = format!("tenant-{unique}");
        let repository_id = format!("repo-{unique}");
        for suffix in ["a", "b"] {
            backing
                .create_task(
                    &NewWorkTask {
                        tenant_id: tenant_id.clone(),
                        repository_id: repository_id.clone(),
                        task_id: format!("task-{unique}-{suffix}"),
                        title: format!("overlapping task {suffix}"),
                        acceptance: "declares a shared path".to_string(),
                        goal_id: None,
                        declared_paths: vec!["crates/shared/src/lib.rs".to_string()],
                        declared_symbols: vec![],
                        published_by: "test-publisher".to_string(),
                    },
                    &format!("event-{unique}-{suffix}"),
                    SystemTime::now(),
                )
                .await
                .expect("create task");
        }

        let service = WorkQueryService::new(backing);
        let response = service
            .get_work_board_doctor(Request::new(v1::WorkBoardDoctorRequest {
                tenant_id,
                repository_id,
            }))
            .await
            .expect("get board doctor")
            .into_inner();

        assert!(
            response
                .findings
                .iter()
                .any(|finding| finding.kind == "declared_scope_overlap"),
            "expected a declared_scope_overlap finding, got: {:?}",
            response.findings
        );
    }
}
