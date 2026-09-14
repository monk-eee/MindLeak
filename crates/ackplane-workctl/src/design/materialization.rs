use std::collections::HashMap;

use serde_json::{json, Value};
use url::Url;

use super::{bounded, confirm, confirmation, one, path_identifier, request, response_json};
use crate::many;

pub(super) fn run(
    command: &str,
    flags: &HashMap<String, Vec<String>>,
    mut url: Url,
) -> Result<Value, String> {
    let design = one(flags, "design-id")?;
    path_identifier("design-id", design)?;
    let constitution = one(flags, "constitution-version-id")?;
    bounded("constitution-version-id", constitution, 256)?;
    let idempotency = one(flags, "idempotency-key")?;
    bounded("idempotency-key", idempotency, 256)?;
    let rationale = one(flags, "rationale")?;
    bounded("rationale", rationale, 8192)?;
    let mut tasks = many(flags, "work-task-id");
    let goals = many(flags, "goal-id");
    if tasks.is_empty() || tasks.len() > 32 || goals.len() > 32 {
        return Err("materialization must name 1-32 Work tasks and at most 32 goals".into());
    }
    for identifier in tasks.iter().chain(&goals) {
        bounded("materialization reference", identifier, 256)?;
    }
    tasks.sort_unstable();
    url.path_segments_mut()
        .map_err(|_| "invalid design path")?
        .push(design);
    let detail = response_json(request("GET", &url, None)?)?;
    if detail["design"]["design_id"] != design
        || !matches!(
            detail["design"]["lifecycle_state"].as_str(),
            Some("accepted" | "materialized")
        )
    {
        return Err("materialization requires the selected design to be explicitly accepted; inspect design show".into());
    }
    url.path_segments_mut()
        .map_err(|_| "invalid materialization path")?
        .push("materializations");
    let body = json!({"idempotency_key":idempotency,"constitution_version_id":constitution,"work_task_ids":tasks,"goal_ids":goals,"rationale":rationale});
    let digest = confirmation("materialize_design", &url, &body)?;
    if command == "materialization-preview" {
        return Ok(
            json!({"operation":"materialize_design","design":detail,"materialization":body,"confirmation_digest":digest,"persisted":false}),
        );
    }
    confirm(one(flags, "confirm-digest")?, &digest)?;
    let revision = response_json(request("POST", &url, Some(&body))?)?;
    if revision["constitution_version_id"] != constitution
        || revision["work_task_ids"] != body["work_task_ids"]
        || revision["goal_ids"] != body["goal_ids"]
        || revision["rationale"] != body["rationale"]
        || revision["revision_number"]
            .as_i64()
            .filter(|number| *number > 0)
            .is_none()
    {
        return Err(
            "Bridge returned a mismatched materialization; inspect the design before retrying"
                .into(),
        );
    }
    Ok(
        json!({"operation":"materialize_design","revision":revision,"persisted":true,"confirmation_digest":digest}),
    )
}
