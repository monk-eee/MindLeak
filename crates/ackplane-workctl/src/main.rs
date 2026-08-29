//! A command-line client for the Bridge's ADR-0125 Work command API
//! (`crates/ackplane-bridge/src/work_command_api.rs`). It is a plain HTTP
//! client of that same versioned, authenticated API -- never a second way to
//! reach `WorkCommandService`, `WorkStore`, or `ClaimStore` directly (the
//! contract violation ADR-0125 decision 11 rejects). Anything this tool can
//! do, a browser calling the identical route can do too; the only thing it
//! adds is a terminal-friendly, scriptable caller.
//!
//! Every command today resolves to a typed `authorization_unavailable`
//! result under the Bridge's loopback developer profile (ADR-0125 decision
//! 2) -- this tool does not, and cannot, work around that; it exists to
//! exercise and script against the real contract, honestly, not to grant
//! itself authority the Bridge itself does not have.
//!
//! Usage:
//! ```text
//! ackplane-workctl submit <kind> --bridge-url URL --repository-id ID [flags...]
//! ackplane-workctl confirm <kind> --bridge-url URL --repository-id ID --command-id ID [flags...]
//! ackplane-workctl help
//! ```

use std::{
    collections::HashMap,
    env,
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{json, Value};

const KINDS: [&str; 10] = [
    "create_work",
    "route_work",
    "release_lease",
    "answer_wait",
    "submit_review",
    "assign",
    "steer",
    "pause",
    "resume",
    "drain",
];

fn usage() -> String {
    format!(
        "usage:\n  \
         ackplane-workctl submit <kind> --bridge-url URL --repository-id ID \
         --issuing-principal-id ID --idempotency-key KEY --rationale TEXT \
         --expires-in-seconds N [--existing-task-id ID] [--expected-task-version N] \
         [--delegation-id ID] [--policy-ref REF ...] <kind-specific flags>\n  \
         ackplane-workctl confirm <kind> --bridge-url URL --repository-id ID \
         --command-id ID <kind-specific flags>\n  \
         ackplane-workctl help\n\nkinds: {}",
        KINDS.join(", ")
    )
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("ackplane-workctl: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let Some((mode, rest)) = args.split_first() else {
        eprintln!("{}", usage());
        return Ok(ExitCode::FAILURE);
    };
    match mode.as_str() {
        "submit" => submit(rest),
        "confirm" => confirm(rest),
        "help" | "-h" | "--help" => {
            println!("{}", usage());
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("unknown command '{other}'\n{}", usage())),
    }
}

/// Every flag may repeat; `one`/`optional` read the last occurrence, `many`
/// reads all of them (used only by `--policy-ref`).
fn parse_flags(args: &[String]) -> Result<HashMap<String, Vec<String>>, String> {
    let mut flags: HashMap<String, Vec<String>> = HashMap::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let Some(name) = arg.strip_prefix("--") else {
            return Err(format!(
                "unexpected argument '{arg}' (flags must start with --)"
            ));
        };
        let value = iter
            .next()
            .ok_or_else(|| format!("--{name} requires a value"))?;
        flags
            .entry(name.to_string())
            .or_default()
            .push(value.clone());
    }
    Ok(flags)
}

fn one<'a>(flags: &'a HashMap<String, Vec<String>>, name: &str) -> Result<&'a str, String> {
    flags
        .get(name)
        .and_then(|values| values.last())
        .map(String::as_str)
        .ok_or_else(|| format!("--{name} is required"))
}

fn optional<'a>(flags: &'a HashMap<String, Vec<String>>, name: &str) -> Option<&'a str> {
    flags
        .get(name)
        .and_then(|values| values.last())
        .map(String::as_str)
}

fn many(flags: &HashMap<String, Vec<String>>, name: &str) -> Vec<String> {
    flags.get(name).cloned().unwrap_or_default()
}

fn parse_u64(flags: &HashMap<String, Vec<String>>, name: &str) -> Result<u64, String> {
    one(flags, name)?
        .parse()
        .map_err(|_| format!("--{name} must be a non-negative integer"))
}

fn parse_bool(flags: &HashMap<String, Vec<String>>, name: &str) -> Result<bool, String> {
    match one(flags, name)? {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("--{name} must be 'true' or 'false', got '{other}'")),
    }
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock must be after the Unix epoch")
        .as_secs()
}

/// The kind-specific fields for one command, merged into the request body
/// alongside its `kind` tag. Field names match
/// `work_command_api::WorkCommandPayloadRequest` exactly.
fn merge_payload_fields(
    body: &mut Value,
    kind: &str,
    flags: &HashMap<String, Vec<String>>,
) -> Result<(), String> {
    body["kind"] = json!(kind);
    match kind {
        "create_work" => {
            body["task_id"] = json!(one(flags, "task-id")?);
            body["title"] = json!(one(flags, "title")?);
            body["acceptance"] = json!(one(flags, "acceptance")?);
            if let Some(goal_id) = optional(flags, "goal-id") {
                body["goal_id"] = json!(goal_id);
            }
        }
        "route_work" => {
            body["route_reference"] = json!(one(flags, "route-reference")?);
        }
        "release_lease" => {
            body["expected_owner_id"] = json!(one(flags, "expected-owner-id")?);
            body["expected_lease_expires_at_seconds"] =
                json!(parse_u64(flags, "expected-lease-expires-at-seconds")?);
        }
        "answer_wait" => {
            body["wait_id"] = json!(one(flags, "wait-id")?);
            body["answer"] = json!(one(flags, "answer")?);
        }
        "submit_review" => {
            body["disposition"] = json!(one(flags, "disposition")?);
            body["review_rationale"] = json!(one(flags, "review-rationale")?);
        }
        "assign" | "resume" => {
            body["target_node_id"] = json!(one(flags, "target-node-id")?);
            body["target_session_id"] = json!(one(flags, "target-session-id")?);
        }
        "steer" => {
            body["target_node_id"] = json!(one(flags, "target-node-id")?);
            body["target_session_id"] = json!(one(flags, "target-session-id")?);
            body["instruction"] = json!(one(flags, "instruction")?);
            body["checkpoint_required"] = json!(parse_bool(flags, "checkpoint-required")?);
        }
        "pause" => {
            body["target_node_id"] = json!(one(flags, "target-node-id")?);
            body["target_session_id"] = json!(one(flags, "target-session-id")?);
            body["checkpoint_required"] = json!(parse_bool(flags, "checkpoint-required")?);
        }
        "drain" => {
            body["target_node_id"] = json!(one(flags, "target-node-id")?);
            body["target_session_id"] = json!(one(flags, "target-session-id")?);
            body["deadline_seconds"] = json!(parse_u64(flags, "deadline-seconds")?);
        }
        other => {
            return Err(format!(
                "unknown command kind '{other}' (expected one of: {})",
                KINDS.join(", ")
            ))
        }
    }
    Ok(())
}

/// Sends `body` to `url` and returns the parsed JSON response regardless of
/// HTTP status -- the Bridge's typed outcomes (including refusals) are
/// carried in a `200` body, so only a transport or malformed-response
/// failure is a hard error here.
fn post(url: &str, body: &Value) -> Result<Value, String> {
    match ureq::post(url).send_json(body.clone()) {
        Ok(response) => response
            .into_json()
            .map_err(|error| format!("could not parse the Bridge's response as JSON: {error}")),
        Err(ureq::Error::Status(_, response)) => response.into_json().map_err(|error| {
            format!("could not parse the Bridge's error response as JSON: {error}")
        }),
        Err(error) => Err(format!("could not reach the Bridge at {url}: {error}")),
    }
}

fn submit(args: &[String]) -> Result<ExitCode, String> {
    let Some((kind, rest)) = args.split_first() else {
        return Err(format!("submit requires a command kind\n{}", usage()));
    };
    if !KINDS.contains(&kind.as_str()) {
        return Err(format!("unknown command kind '{kind}'\n{}", usage()));
    }
    let flags = parse_flags(rest)?;
    let bridge_url = one(&flags, "bridge-url")?;
    let repository_id = one(&flags, "repository-id")?;
    let issuing_principal_id = one(&flags, "issuing-principal-id")?;
    let idempotency_key = one(&flags, "idempotency-key")?;
    let rationale = one(&flags, "rationale")?;
    let expires_in_seconds = parse_u64(&flags, "expires-in-seconds")?;
    let existing_task_id = optional(&flags, "existing-task-id");
    if kind != "create_work" && existing_task_id.is_none() {
        return Err(format!("--existing-task-id is required for '{kind}'"));
    }
    let expected_task_version = optional(&flags, "expected-task-version")
        .map(|value| {
            value
                .parse::<i64>()
                .map_err(|_| "--expected-task-version must be an integer".to_string())
        })
        .transpose()?;
    let delegation_id = optional(&flags, "delegation-id");
    let policy_refs = many(&flags, "policy-ref");

    let mut body = json!({
        "issuing_principal_id": issuing_principal_id,
        "idempotency_key": idempotency_key,
        "rationale": rationale,
        "policy_refs": policy_refs,
        "expires_at_seconds": now_unix_seconds() + expires_in_seconds,
    });
    if let Some(existing_task_id) = existing_task_id {
        body["existing_task_id"] = json!(existing_task_id);
    }
    if let Some(expected_task_version) = expected_task_version {
        body["expected_task_version"] = json!(expected_task_version);
    }
    if let Some(delegation_id) = delegation_id {
        body["delegation_id"] = json!(delegation_id);
    }
    merge_payload_fields(&mut body, kind, &flags)?;

    let url = format!("{bridge_url}/api/v1/repositories/{repository_id}/work/commands");
    let response = post(&url, &body)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&response).expect("a JSON value always re-serializes")
    );
    Ok(ExitCode::SUCCESS)
}

fn confirm(args: &[String]) -> Result<ExitCode, String> {
    let Some((kind, rest)) = args.split_first() else {
        return Err(format!("confirm requires a command kind\n{}", usage()));
    };
    if !KINDS.contains(&kind.as_str()) {
        return Err(format!("unknown command kind '{kind}'\n{}", usage()));
    }
    let flags = parse_flags(rest)?;
    let bridge_url = one(&flags, "bridge-url")?;
    let repository_id = one(&flags, "repository-id")?;
    let command_id = one(&flags, "command-id")?;

    let mut body = json!({});
    merge_payload_fields(&mut body, kind, &flags)?;

    let url = format!(
        "{bridge_url}/api/v1/repositories/{repository_id}/work/commands/{command_id}/confirm"
    );
    let response = post(&url, &body)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&response).expect("a JSON value always re-serializes")
    );
    Ok(ExitCode::SUCCESS)
}
