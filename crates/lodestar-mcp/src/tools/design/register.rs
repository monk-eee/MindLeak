//! `design_register`: register one design, or reconcile a batch from
//! repository ADR metadata.

use lodestar_core::design::DesignMetadata;
use lodestar_core::Lodestar;
use serde_json::Value;

use super::super::{ok, opt_str, req_str};

pub(super) fn register(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    if args.get("designs").is_some() {
        if args.get("adr_path").is_some() {
            return Err("design_register takes either one design (adr_path, title) \
                        or a reconcile batch (designs), not both."
                .to_string());
        }
        let designs = parse_design_metadata(args)?;
        return ok(&engine
            .reconcile_designs(&designs)
            .map_err(|error| error.to_string())?);
    }
    ok(&engine
        .register_design(
            req_str(args, "adr_path")?,
            req_str(args, "title")?,
            opt_str(args, "summary").unwrap_or_default().as_str(),
            Some(req_str(args, "agent")?),
        )
        .map_err(|error| error.to_string())?)
}

fn parse_design_metadata(args: &Value) -> Result<Vec<DesignMetadata>, String> {
    let designs = args
        .get("designs")
        .cloned()
        .ok_or_else(|| "missing required array arg: designs".to_string())?;
    serde_json::from_value(designs).map_err(|error| format!("invalid design metadata: {error}"))
}
