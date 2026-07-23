//! Optional idle maintenance scheduler for autonomous signal consolidation.

mod config;
mod runtime;

pub(crate) use config::MaintenanceConfig;
pub(crate) use runtime::{ActivitySignal, MaintenanceRuntime};
