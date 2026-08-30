//! The tools this front door translates (ADR-0136 clause 1).
//!
//! Every handler here is a translation to an Ackplane RPC that already exists
//! and is already authorized. Nothing in this module decides whether a node is
//! enrolled, what a state means, or how a signature verifies -- that authority
//! stays in `ackplane-server`, which is the whole point of a front door rather
//! than a second storage core (ADR-0136 clause 6).

use ackplane_client::identity::{
    load_candidate_identity, resolve_key_path, signed_status_request, IdentityError,
};
use ackplane_client::{
    ActiveClaimsRequest, ClaimClient, EnrollmentClient, ListWorkTasksRequest,
    WorkBoardDoctorRequest, WorkQueryClient, WorkTaskDetailRequest,
};
use ackplane_protocol::v1::EnrollmentState;
use mindleak_session::{SessionContext, SessionRegistry};
use serde_json::{json, Value};

/// Named for the RPC it translates rather than for a local tool, because
/// Ackplane's enrolment question has no local-vocabulary equivalent to track
/// (ADR-0136 clause 2).
pub const CHECK_ENROLLMENT_STATUS: &str = "check_enrollment_status";

/// Reads Ackplane's own arbitration (ADR-0139 clause 1's read half). Named
/// distinctly from Lodestar's `task_query` because it answers a narrower
/// question -- which claims this arbiter holds -- rather than the local board's.
pub const ACTIVE_CLAIMS: &str = "active_claims";

/// ADR-0139 clause 2: composes ADR-0120's existing read-only Work
/// projection -- list, detail, and Board Doctor findings -- exactly as
/// Bridge's first Work read surface already does. Named the same as
/// Lodestar's own `task_query` because it answers the analogous question,
/// scoped to what Ackplane's `work_tasks` domain actually supports (ADR-0139
/// clause 6 discloses the narrowing rather than hiding it).
pub const TASK_QUERY: &str = "task_query";

/// ADR-0137 clause 2: identity is the session, not the process, exactly as it
/// already is on `mindleak-mcp`/`lodestar-mcp`. Node-level connection trust
/// (`endpoint::resolve_endpoint`'s loopback pilot) authenticates *this
/// process*; `open_session` layers an independently-declared agent identity
/// on top, distinguishing concurrent callers behind one long-lived front door.
pub const OPEN_SESSION: &str = "open_session";

const TENANT_ID_ENV: &str = "MINDLEAK_ACKPLANE_TENANT_ID";
const REPOSITORY_ID_ENV: &str = "MINDLEAK_ACKPLANE_REPOSITORY_ID";

/// Names ADR-0139 clause 3 refuses outright, with the reason it refuses them.
/// Listed rather than pattern-matched so adding one is a deliberate act.
const DEFERRED_BY_ADR_0120: &[&str] = &["task_create", "task_transition"];

pub fn advertised() -> Vec<Value> {
    vec![
        json!({
            "name": OPEN_SESSION,
            "description":
                "Register this MCP client session's identity, exactly as open_session already \
                 works on mindleak-mcp/lodestar-mcp (ADR-0030, amended by ADR-0054): identity is \
                 the session, not the process, so one long-lived ackplane-mcp process serves \
                 multiple concurrent callers distinguished only by their registered session_id. \
                 Optional working-context fields (branch, head_sha, base, dirty, behind) are \
                 declared by the client and never detected by the server (ADR-0137).",
            "inputSchema": mindleak_session::session_input_schema()
        }),
        json!({
            "name": CHECK_ENROLLMENT_STATUS,
            "description":
                "Ask Ackplane whether this repository's candidate (node, key) binding is enrolled \
                 right now. Translates NodeEnrollmentService.CheckEnrollmentStatus (ADR-0122); the \
                 verdict is the arbiter's, never recomputed here.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": ACTIVE_CLAIMS,
            "description":
                "List the claims Ackplane currently arbitrates for this repository, as it sees \
                 them. Translates ClaimDelegationService.ListActiveClaims (ADR-0096); read-only, \
                 grants no authority, and reports the arbiter's view rather than any local board's.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": TASK_QUERY,
            "description":
                "Read Ackplane's own Industrial Work projection for this repository (ADR-0139 \
                 clause 2): a paged/filterable task list, task detail and event history, declared \
                 scope/overlap and stalled/waiting work (Board Doctor), following ADR-0112's \
                 bounded pagination discipline. `view` selects the question: `list` (optionally \
                 filtered by `state`, paged by `page`/`page_size`), `detail` (requires `task_id`; \
                 returns acceptance, event history, and waits), or `doctor` (Board Doctor \
                 findings). Every `list` answer carries ADR-0120 decision 6's publication state \
                 (`current`/`claims_only`/`not_published` today; `lagging`/`unavailable` are not \
                 yet computed by any read surface). Read-only: this tool exposes no create, \
                 route, or lifecycle-mutation operation, because ADR-0120 decision 8 defers all \
                 of them -- this is materially narrower than Lodestar's own task_query \
                 (ADR-0139 clause 6).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "view": { "type": "string", "enum": ["list", "detail", "doctor"] },
                    "task_id": { "type": "string" },
                    "state": { "type": "string" },
                    "page": { "type": "integer" },
                    "page_size": { "type": "integer" }
                },
                "required": ["view"],
                "additionalProperties": false
            }
        }),
    ]
}

/// Register (or re-register) this session's identity (ADR-0137 clause 2).
///
/// Intercepted by the server ahead of [`call`] -- exactly how `mindleak-mcp`
/// and `lodestar-mcp` already special-case `open_session` -- because it needs
/// the session registry, not an Ackplane endpoint: no arbiter is consulted to
/// open a session, matching the local planes' contract byte for byte.
pub fn open_session(sessions: &SessionRegistry, arguments: &Value) -> Result<Value, String> {
    let token = arguments
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing required string arg: session_id".to_string())?;
    let context = SessionContext::from_arguments(arguments)?;
    let identity = sessions.open_session(token, context)?;
    let mut body = json!({ "agent_id": identity.agent_id });
    if let Some(context) = identity.context.declared_json() {
        body["context"] = context;
    }
    Ok(body)
}

/// Dispatch one tool call, or say plainly that the name is not served here.
///
/// [`OPEN_SESSION`] is deliberately absent from this match: the server
/// intercepts it before ever calling here (see [`open_session`]'s doc
/// comment), so it never reaches the "unknown tool" branch below despite
/// being one of [`advertised`]'s four names.
pub fn call<F>(
    endpoint: &str,
    name: &str,
    arguments: &Value,
    environment: &F,
) -> Result<Value, String>
where
    F: Fn(&str) -> Option<String>,
{
    match name {
        CHECK_ENROLLMENT_STATUS => check_enrollment_status(endpoint, environment),
        ACTIVE_CLAIMS => active_claims(endpoint, environment),
        TASK_QUERY => task_query(endpoint, arguments, environment),
        // ADR-0139 clause 3: Ackplane does not accept these, so neither does its
        // front door. Named separately from an unknown tool because the reason
        // is different and actionable -- the operation exists, the authority
        // does not.
        deferred if DEFERRED_BY_ADR_0120.contains(&deferred) => Err(format!(
            "Industrial Work does not yet accept `{deferred}`; see ADR-0120 decision 8. This front \
             door reflects Ackplane's actual authority, so it refuses rather than approximating \
             the operation client-side and reporting a state Ackplane never recorded."
        )),
        other => Err(format!(
            "unknown tool: {other}. This front door serves {OPEN_SESSION}, \
             {CHECK_ENROLLMENT_STATUS}, {ACTIVE_CLAIMS}, and {TASK_QUERY}; it refuses a name it \
             does not translate rather than approximating one."
        )),
    }
}

/// Read this arbiter's own view of the claims it holds for this repository.
///
/// The request carries no `ClaimAuthentication` -- `ListActiveClaims` asks what
/// the arbiter already states about its own arbitration and grants nothing, so
/// no signer is needed and none is used.
fn active_claims<F>(endpoint: &str, environment: &F) -> Result<Value, String>
where
    F: Fn(&str) -> Option<String>,
{
    let tenant_id = required(environment, TENANT_ID_ENV)?;
    let repository_id = required(environment, REPOSITORY_ID_ENV)?;

    let result = runtime()?
        .block_on(async {
            ClaimClient::connect(endpoint)
                .await?
                .list_active_claims(ActiveClaimsRequest {
                    tenant_id,
                    repository_id,
                })
                .await
        })
        .map_err(|error| format!("could not reach Ackplane at {endpoint}: {error}"))?;

    Ok(json!({
        "claims": result
            .claims
            .into_iter()
            .map(|claim| json!({
                "task_id": claim.task_id,
                "owner_id": claim.owner_id,
                "branch": claim.branch,
                "lease_expires_at": claim.lease_expires_at,
                "paths": claim.paths,
                "symbols": claim.symbols,
            }))
            .collect::<Vec<_>>()
    }))
}

/// Compose ADR-0120's read-only Work projection (ADR-0139 clause 2). `view`
/// selects which of `WorkQueryService`'s three RPCs answers the question;
/// every branch requires the same tenant/repository scope `active_claims`
/// already does, refused unset for the identical reason (an empty scope
/// reads exactly like "this repository has no work").
fn task_query<F>(endpoint: &str, arguments: &Value, environment: &F) -> Result<Value, String>
where
    F: Fn(&str) -> Option<String>,
{
    let tenant_id = required(environment, TENANT_ID_ENV)?;
    let repository_id = required(environment, REPOSITORY_ID_ENV)?;
    let view = arguments
        .get("view")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing required string arg: view (list, detail, or doctor)".to_string())?;

    match view {
        "list" => list_work_tasks(endpoint, arguments, tenant_id, repository_id),
        "detail" => get_work_task_detail(endpoint, arguments, tenant_id, repository_id),
        "doctor" => get_work_board_doctor(endpoint, tenant_id, repository_id),
        other => Err(format!(
            "unrecognised task_query view: {other}. This tool serves list, detail, and doctor."
        )),
    }
}

fn list_work_tasks(
    endpoint: &str,
    arguments: &Value,
    tenant_id: String,
    repository_id: String,
) -> Result<Value, String> {
    let state = arguments
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let page = arguments.get("page").and_then(Value::as_i64).unwrap_or(0);
    let page_size = arguments
        .get("page_size")
        .and_then(Value::as_i64)
        .unwrap_or(0);

    let result = runtime()?
        .block_on(async {
            WorkQueryClient::connect(endpoint)
                .await?
                .list_work_tasks(ListWorkTasksRequest {
                    tenant_id,
                    repository_id,
                    state,
                    page,
                    page_size,
                })
                .await
        })
        .map_err(|error| format!("could not reach Ackplane at {endpoint}: {error}"))?;

    Ok(json!({
        "items": result.items.into_iter().map(work_task_to_json).collect::<Vec<_>>(),
        "total": result.total,
        "page": result.page,
        "page_size": result.page_size,
        "publication": result.publication.map(publication_to_json),
    }))
}

fn get_work_task_detail(
    endpoint: &str,
    arguments: &Value,
    tenant_id: String,
    repository_id: String,
) -> Result<Value, String> {
    let task_id = arguments
        .get("task_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "missing required string arg: task_id (required for view=detail)".to_string()
        })?
        .to_string();

    let result = runtime()?
        .block_on(async {
            WorkQueryClient::connect(endpoint)
                .await?
                .get_work_task_detail(WorkTaskDetailRequest {
                    tenant_id,
                    repository_id,
                    task_id,
                })
                .await
        })
        .map_err(|error| format!("could not reach Ackplane at {endpoint}: {error}"))?;

    Ok(json!({
        "task": result.task.map(work_task_to_json),
        "acceptance": result.acceptance,
        "history": result.history.into_iter().map(|event| json!({
            "event_id": event.event_id,
            "from_state": event.from_state,
            "to_state": event.to_state,
            "actor_id": event.actor_id,
            "recorded_at": event.recorded_at,
        })).collect::<Vec<_>>(),
        "waits": result.waits.into_iter().map(|wait| json!({
            "wait_id": wait.wait_id,
            "question": wait.question,
            "audience": wait.audience,
            "asked_by": wait.asked_by,
            "asked_at": wait.asked_at,
            "answered_by": wait.answered_by,
            "answer": wait.answer,
            "answered_at": wait.answered_at,
        })).collect::<Vec<_>>(),
    }))
}

fn get_work_board_doctor(
    endpoint: &str,
    tenant_id: String,
    repository_id: String,
) -> Result<Value, String> {
    let result = runtime()?
        .block_on(async {
            WorkQueryClient::connect(endpoint)
                .await?
                .get_work_board_doctor(WorkBoardDoctorRequest {
                    tenant_id,
                    repository_id,
                })
                .await
        })
        .map_err(|error| format!("could not reach Ackplane at {endpoint}: {error}"))?;

    Ok(json!({
        "findings": result.findings.into_iter().map(|finding| json!({
            "kind": finding.kind,
            "task_id": finding.task_id,
            "detail": finding.detail,
            "related_task_id": finding.related_task_id,
            "title": finding.title,
            "goal_id": finding.goal_id,
            "path": finding.path,
            "wait_id": finding.wait_id,
            "question": finding.question,
            "owner_id": finding.owner_id,
            "since": finding.since,
        })).collect::<Vec<_>>(),
    }))
}

fn work_task_to_json(task: ackplane_client::WorkTaskSummary) -> Value {
    json!({
        "task_id": task.task_id,
        "title": task.title,
        "goal_id": task.goal_id,
        "state": task.state,
        "owner_id": task.owner_id,
        "owner_session_id": task.owner_session_id,
        "lease_expires_at": task.lease_expires_at,
        "declared_paths": task.declared_paths,
        "declared_symbols": task.declared_symbols,
        "published_by": task.published_by,
        "created_at": task.created_at,
        "updated_at": task.updated_at,
    })
}

/// ADR-0120 decision 6's publication-state honesty: names the state
/// (`current`/`claims_only`/`not_published` today -- see this crate's
/// `ackplane-server::work_query_service` for why `lagging`/`unavailable`
/// are not yet computed) rather than presenting an empty task list as a
/// silent "no work".
fn publication_to_json(publication: ackplane_client::WorkPublicationSummary) -> Value {
    json!({
        "state": publication.state,
        "claims_only_total": publication.claims_only_total,
        "claims_only": publication.claims_only.into_iter().map(|claim| json!({
            "task_id": claim.task_id,
            "owner_id": claim.owner_id,
            "branch": claim.branch,
            "lease_expires_at": claim.lease_expires_at,
            "paths": claim.paths,
            "symbols": claim.symbols,
        })).collect::<Vec<_>>(),
    })
}

/// A blank setting is a declaration someone forgot to fill in, so it is refused
/// by name rather than sent to the arbiter as an empty scope that would quietly
/// match nothing.
fn required<F>(environment: &F, name: &str) -> Result<String, String>
where
    F: Fn(&str) -> Option<String>,
{
    environment(name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "{name} is not set, so this front door does not know which repository to ask \
                 Ackplane about. Refused rather than asking with a blank scope, which would \
                 return an empty list that reads exactly like `no claims`."
            )
        })
}

fn check_enrollment_status<F>(endpoint: &str, environment: &F) -> Result<Value, String>
where
    F: Fn(&str) -> Option<String>,
{
    let key_path = resolve_key_path(environment);
    let (identity, signing_key) = match load_candidate_identity(&key_path) {
        Ok(loaded) => loaded,
        // A repository that never ran the enrolment ceremony has no identity to
        // ask about. That is a real answer, and a cheaper one than the arbiter
        // could give -- but it is reported as "cannot ask", never as "not
        // enrolled", because this process did not ask anyone (ADR-0136 clause 2).
        Err(IdentityError::NotFound(path)) => {
            return Err(format!(
                "this repository holds no candidate identity at {}, so there is nothing to ask \
                 Ackplane about. Run `register-me request` here first. Reported as unasked \
                 rather than as `not enrolled`: no arbiter was consulted.",
                path.display()
            ))
        }
        Err(error) => {
            return Err(format!(
                "could not load this repository's identity: {error}"
            ))
        }
    };

    let request = signed_status_request(&identity, &signing_key)
        .map_err(|error| format!("could not sign an enrolment status request: {error}"))?;

    let result = runtime()?
        .block_on(async {
            EnrollmentClient::connect(endpoint)
                .await?
                .check_enrollment_status(request)
                .await
        })
        .map_err(|error| format!("could not reach Ackplane at {endpoint}: {error}"))?;

    Ok(rendered(&identity.node_id, result.verified, result.state))
}

/// `state` is meaningful only when `verified` is true (ADR-0122 decision 5), so
/// an unverified answer deliberately does not report one -- naming a state
/// there would invent a distinction the RPC refuses to draw.
fn rendered(node_id: &str, verified: bool, state: i32) -> Value {
    let mut answer = json!({ "node_id": node_id, "verified": verified });
    if verified {
        answer["state"] = Value::String(
            EnrollmentState::try_from(state)
                .map(|state| state.as_str_name().to_string())
                .unwrap_or_else(|_| format!("unrecognised state {state}")),
        );
    }
    answer
}

/// Shared with [`crate::node_trust`]: a fresh, single-threaded runtime to
/// reach Ackplane from a blocking stdio call. One runtime per call rather
/// than one for the process's lifetime, matching this front door's synchronous
/// dispatch loop -- there is no ambient async context to reuse.
pub(crate) fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("could not start a runtime to reach Ackplane: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn sessions() -> SessionRegistry {
        SessionRegistry::new("test").unwrap()
    }

    #[test]
    fn the_advertised_surface_is_exactly_this_slice_s_four_tools() {
        let advertised = advertised();
        assert_eq!(advertised.len(), 4);
        assert_eq!(advertised[0]["name"], OPEN_SESSION);
        assert_eq!(advertised[1]["name"], CHECK_ENROLLMENT_STATUS);
        assert_eq!(advertised[2]["name"], ACTIVE_CLAIMS);
        assert_eq!(advertised[3]["name"], TASK_QUERY);
    }

    /// ADR-0137 clause 2's contract, in one call: a session opens and returns
    /// the same `session:v1:<hex>` identity form the local planes already use.
    #[test]
    fn open_session_returns_the_local_planes_identity_form() {
        let body = open_session(
            &sessions(),
            &json!({ "session_id": "0123456789abcdef0123456789abcdef" }),
        )
        .expect("a well-formed token opens a session");
        assert!(
            body["agent_id"]
                .as_str()
                .expect("agent_id is a string")
                .starts_with("session:v1:"),
            "got: {body}"
        );
        assert!(body.get("context").is_none(), "nothing was declared");
    }

    #[test]
    fn open_session_reports_the_declared_working_context() {
        let body = open_session(
            &sessions(),
            &json!({
                "session_id": "0123456789abcdef0123456789abcdef",
                "branch": "feat/x",
                "head_sha": "abc123"
            }),
        )
        .expect("a declared context is accepted");
        assert_eq!(body["context"]["branch"], "feat/x");
        assert_eq!(body["context"]["head_sha"], "abc123");
    }

    #[test]
    fn open_session_without_a_token_is_refused_by_name() {
        let error = open_session(&sessions(), &json!({})).expect_err("session_id is required");
        assert!(error.contains("session_id"), "got: {error}");
    }

    /// ADR-0139 clause 3: Ackplane does not accept these, so the front door
    /// refuses by name and says why. Distinct from an unknown tool, because the
    /// operation exists and only the authority is missing -- a client that gets
    /// "unknown tool" would reasonably conclude it had the name wrong.
    #[test]
    fn an_operation_ackplane_defers_is_refused_with_its_reason_not_as_an_unknown_name() {
        for deferred in DEFERRED_BY_ADR_0120 {
            let error = call("http://127.0.0.1:8443", deferred, &Value::Null, &no_env)
                .expect_err("an operation Ackplane defers must be refused");
            assert!(
                error.contains("ADR-0120 decision 8"),
                "{deferred} must name the decision that defers it, got: {error}"
            );
            assert!(
                !error.contains("unknown tool"),
                "{deferred} exists; only the authority is missing, got: {error}"
            );
        }
    }

    /// An empty scope is not a narrower question, it is a different one: the
    /// arbiter would answer with an empty list that reads exactly like
    /// "this repository has no claims".
    #[test]
    fn a_missing_repository_scope_is_refused_rather_than_asked_with_blanks() {
        let only_tenant = |name: &str| (name == TENANT_ID_ENV).then(|| "tenant-a".to_string());
        let error = call(
            "http://127.0.0.1:8443",
            ACTIVE_CLAIMS,
            &Value::Null,
            &only_tenant,
        )
        .expect_err("an unset repository id must be refused");
        assert!(error.contains(REPOSITORY_ID_ENV), "got: {error}");
        assert!(error.contains("reads exactly like"), "got: {error}");
    }

    #[test]
    fn a_blank_scope_is_treated_as_unset_rather_than_as_an_empty_repository() {
        let blank = |name: &str| {
            matches!(name, TENANT_ID_ENV | REPOSITORY_ID_ENV).then(|| "   ".to_string())
        };
        assert!(
            call("http://127.0.0.1:8443", ACTIVE_CLAIMS, &Value::Null, &blank)
                .expect_err("a blank scope must be refused")
                .contains(TENANT_ID_ENV)
        );
    }

    /// An unserved name must be refused by name. Silently succeeding, or
    /// answering a neighbouring tool, is the "fake success" ADR-0136 clause 2
    /// rules out.
    #[test]
    fn an_unserved_tool_name_is_refused_and_says_what_is_served() {
        let error = call("http://127.0.0.1:8443", "recall", &Value::Null, &no_env)
            .expect_err("recall is not translated by this slice");
        assert!(error.contains("unknown tool: recall"), "got: {error}");
        assert!(error.contains(CHECK_ENROLLMENT_STATUS), "got: {error}");
        assert!(error.contains(TASK_QUERY), "got: {error}");
    }

    /// Reported as unasked, not as a negative verdict: no arbiter was consulted,
    /// and "we never asked" and "the answer was no" are different facts.
    #[test]
    fn a_repository_with_no_identity_reports_that_it_cannot_ask_rather_than_not_enrolled() {
        let environment = |name: &str| {
            (name == ackplane_client::identity::KEY_PATH_ENV)
                .then(|| "this/path/does/not/exist.key".to_string())
        };
        let error = call(
            "http://127.0.0.1:8443",
            CHECK_ENROLLMENT_STATUS,
            &Value::Null,
            &environment,
        )
        .expect_err("a repository without an identity cannot ask");
        assert!(
            error.contains("nothing to ask Ackplane about"),
            "got: {error}"
        );
        assert!(error.contains("no arbiter was consulted"), "got: {error}");
    }

    /// `view` is the only way to ask a question of this tool; an absent one is
    /// refused rather than defaulting to a guess about which question was meant.
    #[test]
    fn task_query_without_a_view_is_refused_by_name() {
        let both = |name: &str| {
            matches!(name, TENANT_ID_ENV | REPOSITORY_ID_ENV).then(|| "a-value".to_string())
        };
        let error = call("http://127.0.0.1:8443", TASK_QUERY, &json!({}), &both)
            .expect_err("view is required");
        assert!(error.contains("view"), "got: {error}");
    }

    /// Same discipline as `active_claims`: an unset repository scope is
    /// refused before ever reaching the arbiter, for every view this tool
    /// serves, not only the default one.
    #[test]
    fn task_query_without_a_repository_scope_is_refused_rather_than_asked_with_blanks() {
        let only_tenant = |name: &str| (name == TENANT_ID_ENV).then(|| "tenant-a".to_string());
        for view in ["list", "detail", "doctor"] {
            let error = call(
                "http://127.0.0.1:8443",
                TASK_QUERY,
                &json!({ "view": view }),
                &only_tenant,
            )
            .expect_err("an unset repository id must be refused");
            assert!(
                error.contains(REPOSITORY_ID_ENV),
                "view={view} got: {error}"
            );
        }
    }

    /// `detail` answers a specific task's question; without naming one, this
    /// tool refuses rather than guessing which task was meant.
    #[test]
    fn task_query_detail_without_a_task_id_is_refused_by_name() {
        let both = |name: &str| {
            matches!(name, TENANT_ID_ENV | REPOSITORY_ID_ENV).then(|| "a-value".to_string())
        };
        let error = call(
            "http://127.0.0.1:8443",
            TASK_QUERY,
            &json!({ "view": "detail" }),
            &both,
        )
        .expect_err("task_id is required for view=detail");
        assert!(error.contains("task_id"), "got: {error}");
    }

    /// An unrecognised `view` is refused by name, not silently mapped onto
    /// `list` or treated as an unknown tool.
    #[test]
    fn task_query_with_an_unrecognised_view_is_refused_by_name() {
        let both = |name: &str| {
            matches!(name, TENANT_ID_ENV | REPOSITORY_ID_ENV).then(|| "a-value".to_string())
        };
        let error = call(
            "http://127.0.0.1:8443",
            TASK_QUERY,
            &json!({ "view": "not-a-real-view" }),
            &both,
        )
        .expect_err("an unrecognised view must be refused");
        assert!(error.contains("not-a-real-view"), "got: {error}");
    }

    #[test]
    fn an_unverified_answer_reports_no_state() {
        assert_eq!(
            rendered("node-a", false, EnrollmentState::Active as i32),
            json!({ "node_id": "node-a", "verified": false })
        );
    }

    #[test]
    fn a_verified_answer_names_the_state_the_arbiter_returned() {
        assert_eq!(
            rendered("node-a", true, EnrollmentState::Active as i32),
            json!({
                "node_id": "node-a",
                "verified": true,
                "state": EnrollmentState::Active.as_str_name()
            })
        );
    }

    /// A state this build does not know is reported as the number it was given
    /// rather than collapsed into a known variant, so a newer server widening
    /// the enum cannot be silently misread as something familiar.
    #[test]
    fn an_unrecognised_state_is_reported_as_itself() {
        let answer = rendered("node-a", true, i32::MAX);
        assert_eq!(answer["state"], format!("unrecognised state {}", i32::MAX));
    }
}
