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
//! including multi-runtime `--workers` configuration and recovery boundaries.

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

    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let workers = match arguments.as_slice() {
        [] => None,
        [flag] if flag == "--help" || flag == "-h" => {
            println!("usage: ackplane-supervisor [--workers <workers.json>]\nWorker definitions name an executable, argument vector with one {{prompt}}, absolute working_directory, and branch. Enrollment and endpoint use the MINDLEAK_ACKPLANE_* environment variables.");
            return ExitCode::SUCCESS;
        }
        [flag, path] if flag == "--workers" => match std::fs::read_to_string(path) {
            Ok(json) if json.len() <= 64 * 1024 => Some(json),
            Ok(_) => {
                eprintln!("ackplane-supervisor: worker configuration exceeds 64 KiB");
                return ExitCode::FAILURE;
            }
            Err(error) => {
                eprintln!("ackplane-supervisor: could not read workers file: {error}");
                return ExitCode::FAILURE;
            }
        },
        _ => {
            eprintln!("usage: ackplane-supervisor [--workers <workers.json>]");
            return ExitCode::FAILURE;
        }
    };
    let config = match config::resolve(|name| {
        if name == config::WORKERS_ENV {
            workers.clone().or_else(|| std::env::var(name).ok())
        } else {
            std::env::var(name).ok()
        }
    }) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("ackplane-supervisor: {error}");
            return ExitCode::FAILURE;
        }
    };

    tracing::info!(
        endpoint = %config.endpoint,
        tenant_id = %config.identity.tenant_id,
        repository_id = %config.identity.repository_id,
        node_id = %config.identity.node_id,
        supervisor_id = %config.supervisor_id,
        state_dir = %config.state_dir.display(),
        "starting the Ackplane supervisor"
    );
    if config.workers.is_empty() {
        tracing::warn!(
            "no workers configured; set ACKPLANE_SUPERVISOR_WORKERS to enable agent execution"
        );
    } else {
        tracing::info!(
            workers = config.workers.len(),
            "starting configured worker runtimes with independent sessions and workspaces"
        );
    }

    let (stop, stopping) = tokio::sync::watch::channel(false);
    let run = daemon::run(&config, RECONNECT_DELAY, stopping);
    tokio::pin!(run);
    let result = tokio::select! {
        result = &mut run => result,
        signal = shutdown_signal() => {
            match signal {
                Ok(()) => {
                    tracing::info!("stopping workers and flushing their durable receipts");
                    stop.send_replace(true);
                    match tokio::time::timeout(Duration::from_secs(30), &mut run).await {
                        Ok(result) => result,
                        Err(_) => Err(daemon::DaemonError::Worker(
                            "shutdown deadline exceeded; unacknowledged receipts and run markers are retained".into(),
                        )),
                    }
                }
                Err(error) => Err(daemon::DaemonError::Worker(format!("shutdown signal handler failed: {error}"))),
            }
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ackplane-supervisor: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn shutdown_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await
}
