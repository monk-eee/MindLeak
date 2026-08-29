//! The one tool this slice translates (ADR-0136 clause 1).
//!
//! Every handler here is a translation to an Ackplane RPC that already exists
//! and is already authorized. Nothing in this module decides whether a node is
//! enrolled, what a state means, or how a signature verifies -- that authority
//! stays in `ackplane-server`, which is the whole point of a front door rather
//! than a second storage core (ADR-0136 clause 6).

use ackplane_client::identity::{
    load_candidate_identity, resolve_key_path, signed_status_request, IdentityError,
};
use ackplane_client::EnrollmentClient;
use ackplane_protocol::v1::EnrollmentState;
use serde_json::{json, Value};

/// Named for the RPC it translates rather than for a local tool, because
/// Ackplane's enrolment question has no local-vocabulary equivalent to track
/// (ADR-0136 clause 2).
pub const CHECK_ENROLLMENT_STATUS: &str = "check_enrollment_status";

pub fn advertised() -> Vec<Value> {
    vec![json!({
        "name": CHECK_ENROLLMENT_STATUS,
        "description":
            "Ask Ackplane whether this repository's candidate (node, key) binding is enrolled \
             right now. Translates NodeEnrollmentService.CheckEnrollmentStatus (ADR-0122); the \
             verdict is the arbiter's, never recomputed here.",
        "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
    })]
}

/// Dispatch one tool call, or say plainly that the name is not served here.
pub fn call<F>(endpoint: &str, name: &str, environment: &F) -> Result<Value, String>
where
    F: Fn(&str) -> Option<String>,
{
    match name {
        CHECK_ENROLLMENT_STATUS => check_enrollment_status(endpoint, environment),
        other => Err(format!(
            "unknown tool: {other}. This front door serves {} so far (ADR-0136 slice 1); it \
             refuses a name it does not translate rather than approximating one.",
            CHECK_ENROLLMENT_STATUS
        )),
    }
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

fn runtime() -> Result<tokio::runtime::Runtime, String> {
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

    #[test]
    fn the_advertised_surface_is_exactly_this_slice_s_one_tool() {
        let advertised = advertised();
        assert_eq!(advertised.len(), 1);
        assert_eq!(advertised[0]["name"], CHECK_ENROLLMENT_STATUS);
    }

    /// An unserved name must be refused by name. Silently succeeding, or
    /// answering a neighbouring tool, is the "fake success" ADR-0136 clause 2
    /// rules out.
    #[test]
    fn an_unserved_tool_name_is_refused_and_says_what_is_served() {
        let error = call("http://127.0.0.1:8443", "recall", &no_env)
            .expect_err("recall is not translated by this slice");
        assert!(error.contains("unknown tool: recall"), "got: {error}");
        assert!(error.contains(CHECK_ENROLLMENT_STATUS), "got: {error}");
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
            &environment,
        )
        .expect_err("a repository without an identity cannot ask");
        assert!(
            error.contains("nothing to ask Ackplane about"),
            "got: {error}"
        );
        assert!(error.contains("no arbiter was consulted"), "got: {error}");
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
