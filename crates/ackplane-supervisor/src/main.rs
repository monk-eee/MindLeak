//! `ackplane-supervisor`: the runnable enrolled supervisor daemon.
//!
//! Run it against the local Compose topology once a node is enrolled:
//!
//! ```text
//! docker compose up -d postgres migrate ackplane
//! cargo run -p ackplane-server --bin register-me -- request  --repo my-repo --node my-node ...
//! cargo run -p ackplane-server --bin register-me -- approve  --request-id ... --admin-database-url ...
//! cargo run -p ackplane-server --bin register-me -- activate --request-id ...
//!
//! export MINDLEAK_ACKPLANE_ENDPOINT=http://127.0.0.1:8443
//! export MINDLEAK_ACKPLANE_TENANT_ID=...       # printed by register-me
//! export MINDLEAK_ACKPLANE_REPOSITORY_ID=my-repo
//! export MINDLEAK_ACKPLANE_NODE_ID=my-node
//! export MINDLEAK_ACKPLANE_SIGNING_KEY_ID=...  # printed by register-me activate
//! export ACKPLANE_SUPERVISOR_ID=supervisor-1
//! cargo run -p ackplane-supervisor
//! ```
//!
//! See `crates/ackplane-supervisor/README.md` for the full walkthrough,
//! including what this daemon deliberately cannot do yet.

use std::{process::ExitCode, time::Duration};

use ackplane_supervisor::{config, daemon};

/// How long to wait before reconnecting a dropped connection. Fixed rather
/// than configurable for now: a supervisor that reconnects too eagerly is a
/// load problem, and one that reconnects too slowly is an availability
/// problem, so this deserves a measured default rather than a knob nobody
/// knows how to set.
const RECONNECT_DELAY: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = match config::resolve(|name| std::env::var(name).ok()) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("ackplane-supervisor: {error}");
            return ExitCode::FAILURE;
        }
    };

    tracing::info!(
        endpoint = %config.endpoint,
        tenant_id = %config.tenant_id,
        repository_id = %config.repository_id,
        node_id = %config.node_id,
        supervisor_id = %config.supervisor_id,
        state_dir = %config.state_dir.display(),
        "starting the Ackplane supervisor"
    );
    tracing::warn!(
        "no worker adapter is wired in, so this supervisor declares only the notify \
         capability: a notification is complete once durably recorded, but any directive \
         needing a worker (prompt, assign, steer, pause, resume, drain, terminate) will be \
         durably receipted as refused (capability missing), never as applied"
    );

    match daemon::run(&config, RECONNECT_DELAY).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ackplane-supervisor: {error}");
            ExitCode::FAILURE
        }
    }
}
