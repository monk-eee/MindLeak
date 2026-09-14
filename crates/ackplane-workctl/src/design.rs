use std::{fs::File, io::Read, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use url::{Host, Url};

use super::{one, optional, parse_flags};

mod materialization;

const MAX_PROPOSAL_BYTES: u64 = 64 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Proposal {
    design_id: String,
    title: String,
    summary: String,
    source_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    constitution_version_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    work_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    evidence_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_label: Option<String>,
}

pub(super) fn run(arguments: &[String]) -> Result<Value, String> {
    let (command, rest) = arguments
        .split_first()
        .ok_or("design requires a subcommand")?;
    let allowed: &[&str] = match command.as_str() {
        "preview" => &["file"],
        "propose" => &["file", "confirm-digest"],
        "show" => &["design-id"],
        "list" => &["state", "page"],
        "decision-preview" => &["design-id", "decision", "rationale"],
        "decide" => &["design-id", "decision", "rationale", "confirm-digest"],
        "materialization-preview" | "materialize" => &[
            "design-id",
            "constitution-version-id",
            "work-task-id",
            "goal-id",
            "idempotency-key",
            "rationale",
            "confirm-digest",
        ],
        _ => return Err(format!("unknown design command: {command}")),
    };
    if rest.is_empty() {
        return Err(format!("unknown design command: {command}"));
    }
    let flags = parse_flags(rest)?;
    for (name, values) in &flags {
        if (!allowed.contains(&name.as_str())
            && !["bridge-url", "repository-id"].contains(&name.as_str()))
            || (values.len() != 1 && !["work-task-id", "goal-id"].contains(&name.as_str()))
        {
            return Err(format!("unexpected or repeated design flag: --{name}"));
        }
    }
    let bridge = bridge_url(one(&flags, "bridge-url")?)?;
    let repository = one(&flags, "repository-id")?;
    path_identifier("repository-id", repository)?;
    let mut url = bridge.clone();
    url.path_segments_mut()
        .map_err(|_| "Bridge origin cannot carry a path")?
        .extend(["api", "v1", "repositories", repository, "designs"]);
    if matches!(command.as_str(), "materialization-preview" | "materialize") {
        return materialization::run(command, &flags, url);
    }
    if command == "list" {
        if let Some(state) = optional(&flags, "state") {
            lifecycle(state)?;
            url.query_pairs_mut().append_pair("lifecycle_state", state);
        }
        if let Some(page) = optional(&flags, "page") {
            if page
                .parse::<u32>()
                .ok()
                .filter(|value| *value > 0)
                .is_none()
            {
                return Err("--page must be a positive integer".into());
            }
            url.query_pairs_mut().append_pair("page", page);
        }
        return response_json(request("GET", &url, None)?);
    }
    if matches!(command.as_str(), "show" | "decision-preview" | "decide") {
        let design_id = one(&flags, "design-id")?;
        path_identifier("design-id", design_id)?;
        url.path_segments_mut()
            .map_err(|_| "invalid design path")?
            .push(design_id);
        let detail = response_json(request("GET", &url, None)?)?;
        if detail["design"]["design_id"] != design_id {
            return Err("Bridge returned a different or malformed design".into());
        }
        if command == "show" {
            return Ok(detail);
        }
        let decision = one(&flags, "decision")?;
        lifecycle(decision)?;
        let rationale = one(&flags, "rationale")?;
        bounded("rationale", rationale, 8192)?;
        let state = detail["design"]["lifecycle_state"]
            .as_str()
            .ok_or("Bridge omitted design state")?;
        lifecycle(state)?;
        let body = json!({"decision_kind": decision, "expected_lifecycle_state": state, "rationale": rationale});
        let digest = confirmation("decide_design", &url, &json!([&detail, &body]))?;
        if command == "decision-preview" {
            return Ok(
                json!({"operation":"decide_design", "design":detail, "decision":body, "confirmation_digest":digest, "persisted":false}),
            );
        }
        confirm(one(&flags, "confirm-digest")?, &digest)?;
        let mut mutation = url.clone();
        mutation
            .path_segments_mut()
            .map_err(|_| "invalid decision path")?
            .push("decisions");
        request("POST", &mutation, Some(&body))?;
        return response_json(request("GET", &url, None)?).map_err(|error| {
            format!("decision sent, read-back failed: {error}; use design show before retrying")
        });
    }
    let path = one(&flags, "file")?;
    let file = File::open(path).map_err(|error| format!("cannot open proposal file: {error}"))?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("proposal must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_PROPOSAL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_PROPOSAL_BYTES {
        return Err("proposal exceeds 64 KiB".into());
    }
    let proposal: Proposal = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid design proposal: {error}"))?;
    path_identifier("design_id", &proposal.design_id)?;
    bounded("title", &proposal.title, 512)?;
    bounded("source_version", &proposal.source_version, 256)?;
    bounded("summary", &proposal.summary, 60 * 1024)?;
    for (name, value) in [
        ("constitution_version_id", &proposal.constitution_version_id),
        ("work_task_id", &proposal.work_task_id),
        ("evidence_id", &proposal.evidence_id),
        ("display_label", &proposal.display_label),
    ] {
        if let Some(value) = value {
            bounded(name, value, 256)?;
        }
    }
    let proposal = serde_json::to_value(proposal).map_err(|error| error.to_string())?;
    let confirmation_digest = confirmation("propose_design", &url, &proposal)?;
    if command == "propose" {
        confirm(one(&flags, "confirm-digest")?, &confirmation_digest)?;
        request("POST", &url, Some(&proposal))?;
        let design_id = proposal["design_id"]
            .as_str()
            .ok_or("proposal omitted design_id")?;
        url.path_segments_mut()
            .map_err(|_| "invalid design path")?
            .push(design_id);
        let detail = response_json(request("GET", &url, None)?).map_err(|error| {
            format!("proposal sent, read-back failed: {error}; retry the same proposal and digest")
        })?;
        if !proposal
            .as_object()
            .ok_or("invalid proposal")?
            .iter()
            .all(|(field, value)| detail["design"].get(field) == Some(value))
        {
            return Err("Bridge read-back differs from the proposal; retain the file and inspect the stored design".into());
        }
        return Ok(
            json!({"operation":"propose_design", "repository_id":repository, "design":detail, "persisted":true, "confirmation_digest":confirmation_digest}),
        );
    }
    Ok(json!({
        "operation": "propose_design", "bridge_url": bridge.as_str(), "repository_id": repository,
        "proposal": proposal, "confirmation_digest": confirmation_digest, "persisted": false
    }))
}

fn confirmation(operation: &str, url: &Url, body: &Value) -> Result<String, String> {
    let bytes = serde_json::to_vec(&json!([
        "mindleak.design-review.v1",
        operation,
        url.as_str(),
        body
    ]))
    .map_err(|error| error.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn confirm(received: &str, expected: &str) -> Result<(), String> {
    if received != expected {
        return Err(
            "confirmation does not match this target and content; preview again before writing"
                .into(),
        );
    }
    Ok(())
}

fn lifecycle(value: &str) -> Result<(), String> {
    if ![
        "proposed",
        "accepted",
        "rejected",
        "deferred",
        "retired",
        "superseded",
        "materialized",
    ]
    .contains(&value)
    {
        return Err("unknown design lifecycle state".into());
    }
    Ok(())
}

fn request(method: &str, url: &Url, body: Option<&Value>) -> Result<ureq::Response, String> {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_secs(10))
        .build();
    let request = agent
        .request(method, url.as_str())
        .set("Accept", "application/json");
    let result = match body {
        Some(body) => request.send_json(body.clone()),
        None => request.call(),
    };
    match result {
        Ok(response) if (200..300).contains(&response.status()) => Ok(response),
        Ok(response) | Err(ureq::Error::Status(_, response)) => Err(format!(
            "Bridge returned HTTP {} for {method}; no success is assumed. Retain the same proposal for retries or inspect the design after an uncertain decision.", response.status()
        )),
        Err(_) => Err(format!("Bridge {method} did not return a response; write outcome may be unknown. Retain the proposal and use design show before retrying a decision.")),
    }
}

fn response_json(response: ureq::Response) -> Result<Value, String> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read Bridge response: {error}"))?;
    if bytes.len() > 1024 * 1024 {
        return Err("Bridge response exceeds 1 MiB".into());
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("Bridge returned invalid JSON: {error}"))
}

fn path_identifier(name: &str, value: &str) -> Result<(), String> {
    bounded(name, value, 256)?;
    if matches!(value, "." | "..") {
        return Err(format!("{name} cannot be a URL dot component"));
    }
    Ok(())
}

fn bounded(name: &str, value: &str, maximum: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > maximum {
        return Err(format!(
            "{name} must contain 1-{maximum} bytes of nonblank text"
        ));
    }
    Ok(())
}

fn bridge_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|_| "invalid Bridge URL")?;
    let loopback = match url.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        Some(Host::Domain(name)) => name == "localhost",
        None => false,
    };
    if !loopback
        || !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("design authoring requires a loopback Bridge origin without credentials, path, query or fragment".into());
    }
    Ok(url)
}
