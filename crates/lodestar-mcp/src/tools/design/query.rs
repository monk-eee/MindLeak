//! `design_query`: read-only views over the design ledger.

use lodestar_core::{DesignStatus, Lodestar};
use serde_json::Value;

use super::super::{bool_arg, ok, one_of, opt_str, required_for};
use super::constants::VIEWS;

pub(super) fn query(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let view = one_of(args, "view", &VIEWS)?;
    match view {
        "board" => ok(&engine.design_board().map_err(|error| error.to_string())?),
        "ledger" => {
            let status = match opt_str(args, "status") {
                Some(value) => Some(
                    DesignStatus::from_tag(&value)
                        .ok_or_else(|| format!("unknown design status: {value}"))?,
                ),
                None => None,
            };
            ok(&engine
                .list_design_items(
                    status,
                    bool_arg(args, "include_retired", false),
                    bool_arg(args, "include_deferred", false),
                )
                .map_err(|error| error.to_string())?)
        }
        "promotion" => ok(&engine
            .design_promotion(&required_for(args, "id", view, "the design to read.")?)
            .map_err(|error| error.to_string())?),
        "history" => ok(&engine
            .design_materialization_history(&required_for(args, "id", view, "the design to read.")?)
            .map_err(|error| error.to_string())?),
        "actions" => ok(&engine
            .design_action_history(&required_for(
                args,
                "id",
                view,
                "the design whose attributed actions should be read.",
            )?)
            .map_err(|error| error.to_string())?),
        other => unreachable!("one_of refused every value but {VIEWS:?}, not {other}"),
    }
}
