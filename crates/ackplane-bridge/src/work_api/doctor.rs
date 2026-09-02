//! ADR-0120 decision 7's Board Doctor findings.

use super::*;

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
pub(super) struct WorkDoctorResponse {
    findings: Vec<WorkDoctorFindingResponse>,
    checked_at_seconds: Option<u64>,
}

pub(super) async fn work_doctor(
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
