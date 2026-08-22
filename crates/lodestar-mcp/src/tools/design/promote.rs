//! `design_promote`: materialise an accepted design into work, in reviewed
//! steps (plan, materialize, revise).

use lodestar_core::Lodestar;
use serde_json::Value;

use super::super::design_materialization::parse_materialization_plan;
use super::super::{ok, one_of, req_str, required_for};
use super::constants::STEPS;

pub(super) fn promote(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let id = req_str(args, "id")?;
    let step = one_of(args, "step", &STEPS)?;
    match step {
        "plan" => ok(&engine
            .plan_design_promotion(
                id,
                &required_for(
                    args,
                    "objective_goal_id",
                    step,
                    "the objective the work hangs under.",
                )?,
            )
            .map_err(|error| error.to_string())?),
        "materialize" => ok(&engine
            .promote_design(id, &parse_materialization_plan(args)?)
            .map_err(|error| error.to_string())?),
        "revise" => ok(&engine
            .revise_design_promotion(
                id,
                &required_for(args, "human", step, "the person recording the repair.")?,
                &parse_materialization_plan(args)?,
            )
            .map_err(|error| error.to_string())?),
        other => unreachable!("one_of refused every value but {STEPS:?}, not {other}"),
    }
}
