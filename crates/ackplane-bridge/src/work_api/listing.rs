//! The paged, filterable Work task list and the command capabilities it reports.

use super::*;

#[derive(Deserialize)]
pub(super) struct WorkListQuery {
    page: Option<i64>,
    page_size: Option<i64>,
    state: Option<String>,
}

#[derive(Serialize)]
pub(super) struct WorkListResponse {
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

pub(super) async fn work_list(
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
