//! Authenticated supervisor connections and concurrent, runtime-neutral worker slots.
//!
//! Configured workers acquire leases, receive scoped context, and report durable
//! effects through the existing inbox/outbox. An unconfigured daemon only accepts
//! notifications. Native processes do not promise checkpoint, pause, or sandboxing.

use std::time::Duration;

use ackplane_client::ClientError;
use ackplane_protocol::supervisor::{
    SupervisorCapabilities, SupervisorDirectiveCapability, SupervisorIdentity,
    SupervisorOutboxDurability, SupervisorRegistration, SupervisorRuntime, SupervisorSession,
    SupervisorWorkerState,
};
use time::OffsetDateTime;

mod claims;
mod delivery;
pub(crate) mod frames;
mod runtime;

use delivery::enqueue_receipt;
pub use delivery::resend_pending;
use frames::{heartbeat_frame, registration_frame, session_frame};

use crate::{
    config::SupervisorConfig,
    reconcile::{reconcile, Reconciliation},
    InboxError, OutboxError,
};

/// How the daemon stopped, so a caller can distinguish an orderly shutdown
/// from a condition that needs a person.
#[derive(Debug)]
pub enum DaemonExit {
    Finished,
    /// The connection closed and the daemon stopped cleanly.
    Disconnected,
    /// Durable local state cannot account for what the server holds. Reported
    /// rather than resumed (ADR-0116 decision 3); an operator decides.
    IncompleteEvidence {
        local_acknowledged: u64,
        local_last_enqueued: u64,
        server_accepted: u64,
    },
}

/// Everything that can stop the daemon before it is running.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("worker runtime: {0}")]
    Worker(String),
    #[error("the supervisor's signing key could not be loaded: {0}")]
    Signer(String),
    #[error("connecting to Ackplane failed: {0}")]
    Connect(#[from] Box<ClientError>),
    #[error("the durable inbox could not be opened: {0}")]
    Inbox(#[from] InboxError),
    #[error("the durable outbox could not be opened: {0}")]
    Outbox(#[from] OutboxError),
    #[error("Ackplane permanently rejected outbox frame {sequence} ({reason:?}): {diagnostic}; queued evidence retained for operator recovery")]
    RejectedFrame {
        sequence: u64,
        reason: ackplane_protocol::v1::RejectionReason,
        diagnostic: String,
    },
    #[error("the supervisor clock is outside the representable range")]
    Clock,
}

/// The runtime this daemon reports itself as. It supervises a worker on the
/// machine it runs on; it does not claim a container or cloud runtime it has
/// no way to verify.
const RUNTIME: SupervisorRuntime = SupervisorRuntime::LocalMachine;
const SLOT_SHUTDOWN_GRACE: Duration = Duration::from_secs(30);

/// This supervisor's registration: an honest declaration of what it can do.
///
/// Notification-only without workers; configured processes add assignment and
/// force termination of owned groups, but no unsupported interactive controls.
pub fn registration(config: &SupervisorConfig, node_id: &str) -> SupervisorRegistration {
    SupervisorRegistration {
        supervisor_id: config.supervisor_id.clone(),
        identity: SupervisorIdentity {
            tenant_id: config.node.tenant_id.clone(),
            repository_id: config.node.repository_id.clone(),
            node_id: node_id.to_string(),
        },
        supervisor_version: env!("CARGO_PKG_VERSION").to_string(),
        protocol_version: "v1".to_string(),
        capabilities: SupervisorCapabilities {
            supported_directives: if config.workers.is_empty() {
                vec![SupervisorDirectiveCapability::Notify]
            } else {
                vec![
                    SupervisorDirectiveCapability::Notify,
                    SupervisorDirectiveCapability::Assign,
                    SupervisorDirectiveCapability::TerminateForce,
                ]
            },
            supports_checkpoint: false,
            supports_force_termination: !config.workers.is_empty(),
            outbox_durability: SupervisorOutboxDurability::Persistent,
            recoverable_outbox: true,
        },
    }
}

/// The worker session this supervisor reports.
pub fn session(
    config: &SupervisorConfig,
    started_at: OffsetDateTime,
) -> Result<SupervisorSession, DaemonError> {
    Ok(SupervisorSession {
        session_id: format!("{}:session", config.supervisor_id),
        supervisor_id: config.supervisor_id.clone(),
        worker_id: format!("{}:worker", config.supervisor_id),
        runtime: RUNTIME,
        started_at: started_at.unix_timestamp(),
        state: SupervisorWorkerState::Started,
    })
}

/// Open one connection, announce this supervisor, and serve directives until
/// the connection closes or local evidence stops adding up.
///
/// One connection per call deliberately: reconnect policy (how long to wait,
/// how many times, whether to give up) is an operator concern, and burying it
/// in the loop would make it untestable and unconfigurable. [`run`] supplies a
/// simple policy over this.
async fn serve_once(
    config: &SupervisorConfig,
    runtime: &mut runtime::WorkerRuntime,
) -> Result<DaemonExit, DaemonError> {
    let started_at = runtime.started_at;
    let positions = runtime.outbox.positions()?;

    let mut connection = match tokio::time::timeout(
        Duration::from_secs(10),
        config.node.open_sync(
            positions.acknowledged,
            Some(ackplane_client::companion::wire::SupervisorScope {
                supervisor_id: runtime.session.supervisor_id.clone(),
                session_id: runtime.session.session_id.clone(),
                worker_id: runtime.session.worker_id.clone(),
            }),
        ),
    )
    .await
    {
        Ok(Ok(connection)) => connection,
        Ok(Err(error)) => {
            return disconnected_on_error::<()>(Err(error)).map(|_| DaemonExit::Disconnected)
        }
        Err(_) => return Ok(DaemonExit::Disconnected),
    };

    // REGISTER, THEN RECONCILE, THEN RESEND -- in that order, deliberately.
    //
    // ADR-0116 decision 7's `IncompleteEvidence` case -- the server holding
    // more supervisor evidence than this node can account for -- was
    // undetectable here until now, and this function said so at length rather
    // than pretending otherwise. `HelloAccepted.accepted_position` is an echo
    // of the `last_accepted_position` the client itself just sent
    // (`service/handshake.rs` carries `hello.last_accepted_position` straight
    // through), so reconciling against it compared a number with its own
    // reflection and could only ever answer `UpToDate`.
    //
    // ADR-0141 decided the server must report its *own* view instead. ADR-0146
    // supplied the inbound half that makes such a view exist: this supervisor
    // stamps its outbox sequence on every outbox-carried frame, and the server
    // records the highest it durably accepted. The answer rides on the receipt
    // for the *registration* frame, because that is the first point at which
    // the server knows which supervisor it is talking to -- `Hello` identifies
    // only `producer_id`. That is why registration now precedes the resend
    // rather than following it.
    let registration_receipt = match connection
        .exchange_supervisor_frame(registration_frame(&runtime.registration))
        .await
    {
        Ok(receipt) => receipt,
        Err(error) => {
            return disconnected_on_error::<()>(Err(error)).map(|_| DaemonExit::Disconnected)
        }
    };

    // An absent position means the server makes no independent statement --
    // an older server, or one that has accepted no sequenced frame from this
    // supervisor. The reconciliation then stays unrun, which is exactly the
    // previous behaviour, rather than being decided against a fabricated zero
    // (ADR-0146 decision 5: both directions degrade to silence, never to a
    // verdict).
    if let Some(server_accepted) = registration_receipt.accepted_outbox_sequence {
        match reconcile(positions, server_accepted) {
            Reconciliation::IncompleteEvidence {
                local_acknowledged,
                local_last_enqueued,
                server_accepted,
            } => {
                tracing::error!(
                    local_acknowledged,
                    local_last_enqueued,
                    server_accepted,
                    "Ackplane's receipt position is outside this outbox's recoverable \
                     interval; stopping rather than resuming on incomplete evidence"
                );
                return Ok(DaemonExit::IncompleteEvidence {
                    local_acknowledged,
                    local_last_enqueued,
                    server_accepted,
                });
            }
            Reconciliation::UpToDate { position } => {
                tracing::debug!(position, "supervisor outbox agrees with Ackplane");
            }
            Reconciliation::Resend {
                resend_from,
                through,
            } => {
                tracing::info!(
                    resend_from,
                    through,
                    "Ackplane is behind this outbox; resending the difference"
                );
            }
        }
    }

    if let Some(exit) = resend_pending(&runtime.outbox, &mut connection).await? {
        return Ok(exit);
    }

    if runtime.acknowledge_finished()? {
        return Ok(DaemonExit::Finished);
    }

    if !runtime.finished {
        if let Some(exit) = disconnected_on_error(
            connection
                .exchange_supervisor_frame(session_frame(&runtime.session, started_at)?)
                .await,
        )? {
            return Ok(exit);
        }
    }

    loop {
        runtime.observe(config).await?;
        // The session frame delivers this session's pending directives ahead
        // of its own receipt, so they are already in hand here.
        while let Some(directive) = connection.next_directive() {
            let receipt = match runtime.dispatch(config, &mut connection, &directive).await {
                Ok(receipt) => receipt,
                Err(error) => {
                    tracing::warn!(
                        directive_id = %directive.directive_id,
                        %error,
                        "directive processing stopped; terminating owned workers rather than continuing without durable receipts"
                    );
                    return Err(error);
                }
            };
            tracing::info!(
                directive_id = %receipt.directive_id,
                status = receipt.status,
                reason = receipt.reason,
                "receipted a directive"
            );
            // Durable before transmitted. The receipt is already recorded in
            // the inbox, but the inbox records what this supervisor *decided*;
            // the outbox records what it still owes the server. Without this,
            // a receipt computed and then lost to a dropped connection depends
            // entirely on the server redelivering the directive to be sent
            // again -- true today, but a guarantee held by the other side of a
            // connection that had just failed.
            enqueue_receipt(&runtime.outbox, receipt)?;
        }

        if let Some(exit) = resend_pending(&runtime.outbox, &mut connection).await? {
            return Ok(exit);
        }
        if runtime.acknowledge_finished()? {
            return Ok(DaemonExit::Finished);
        }
        let interval = if config.workers.is_empty() {
            config.heartbeat_interval
        } else {
            config.heartbeat_interval.min(Duration::from_secs(1))
        };
        tokio::time::sleep(interval).await;
        if let Some(exit) = disconnected_on_error(
            connection
                .exchange_supervisor_frame(heartbeat_frame(&config.supervisor_id))
                .await,
        )? {
            return Ok(exit);
        }
        // Re-announcing the session is what asks for newly issued directives:
        // delivery is bound to the session frame, so this is the poll.
        if !runtime.finished {
            if let Some(exit) = disconnected_on_error(
                connection
                    .exchange_supervisor_frame(session_frame(&runtime.session, started_at)?)
                    .await,
            )? {
                return Ok(exit);
            }
        }
    }
}

/// Treat any transport failure while talking to Ackplane as an ordinary
/// dropped connection (`Disconnected`) rather than a fatal `DaemonError`.
///
/// Registration, the session announcement, and a directive receipt used to
/// propagate this exact failure with `.map_err(Box::new)?` instead, which
/// ended the whole daemon (`run`'s `serve_once(config).await?` has nothing to
/// catch) rather than reconnecting -- even though nothing about their
/// connection differs from the heartbeat's, which already treated the
/// identical condition as a plain, retriable disconnect. A supervisor whose
/// connection happened to drop while registering, announcing a session, or
/// submitting a receipt therefore stopped permanently instead of retrying,
/// for no reason tied to what actually failed.
fn disconnected_on_error<T>(
    result: Result<T, ClientError>,
) -> Result<Option<DaemonExit>, DaemonError> {
    match result {
        Ok(_) => Ok(None),
        Err(
            error @ (ClientError::Companion(_)
            | ClientError::Signing(_)
            | ClientError::ConnectionRefused {
                retryable: false, ..
            }),
        ) => Err(Box::new(error).into()),
        Err(error) => {
            tracing::info!(%error, "the supervisor connection closed");
            Ok(Some(DaemonExit::Disconnected))
        }
    }
}

/// Serve, reconnecting on a dropped connection until evidence stops adding up.
///
/// `IncompleteEvidence` deliberately stops the daemon instead of retrying:
/// reconnecting cannot restore a durable record that is already gone, so a
/// retry loop would turn a reportable condition into an invisible one.
pub async fn run(
    config: &SupervisorConfig,
    reconnect_delay: Duration,
    stopping: tokio::sync::watch::Receiver<bool>,
) -> Result<(), DaemonError> {
    let _state_ownership = crate::storage::claim_state_directory(&config.state_dir)
        .map_err(|error| DaemonError::Worker(error.to_string()))?;
    if config.workers.is_empty() {
        return run_session(config, reconnect_delay, stopping, None).await;
    }
    let mut directories = std::collections::HashSet::new();
    for (name, command) in &config.workers {
        let run_path = config.worker_run_path(name);
        if run_path.exists() {
            return Err(DaemonError::Worker(format!(
                "worker {name} has an unaccounted previous run at {}; inspect its processes and durable receipts before recovery",
                run_path.display(),
            )));
        }
        let path = command
            .working_directory
            .canonicalize()
            .map_err(|error| DaemonError::Worker(error.to_string()))?;
        let metadata = path
            .metadata()
            .map_err(|error| DaemonError::Worker(error.to_string()))?;
        if !metadata.is_dir() {
            return Err(DaemonError::Worker(format!(
                "worker {name} working directory is not a directory: {}",
                path.display(),
            )));
        }
        if !directories.insert(path) {
            return Err(DaemonError::Worker(
                "worker directories resolve to the same workspace".into(),
            ));
        }
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut stopping = stopping;
            let (stop_workers, worker_stopping) = tokio::sync::watch::channel(*stopping.borrow());
            let mut workers: tokio::task::JoinSet<Result<(), DaemonError>> =
                tokio::task::JoinSet::new();
            for (name, command) in &config.workers {
                let mut config = config.clone();
                let name = name.clone();
                let command = command.clone();
                let stopping = worker_stopping.clone();
                let stop_workers = stop_workers.clone();
                workers.spawn_local(async move {
                    let base = config.supervisor_id.clone();
                    while !*stopping.borrow() {
                        let mut nonce = [0_u8; 8];
                        getrandom::getrandom(&mut nonce)
                            .map_err(|error| DaemonError::Worker(error.to_string()))?;
                        let suffix = nonce
                            .iter()
                            .map(|byte| format!("{byte:02x}"))
                            .collect::<String>();
                        config.supervisor_id = format!("{base}-{name}-{suffix}");
                        config.workers = [(name.clone(), command.clone())].into_iter().collect();
                        run_session(
                            &config,
                            reconnect_delay,
                            stopping.clone(),
                            Some(&stop_workers),
                        )
                        .await?;
                    }
                    Ok(())
                });
            }
            let mut first_error = None;
            let mut shutdown_deadline = None;
            while !workers.is_empty() {
                let result = if let Some(deadline) = shutdown_deadline {
                    match tokio::time::timeout_at(deadline, workers.join_next()).await {
                        Ok(result) => result,
                        Err(_) => {
                            tracing::error!(remaining_slots = workers.len(), "worker shutdown deadline exceeded; unfinished recovery evidence is retained");
                            return Err(first_error.unwrap_or_else(|| DaemonError::Worker(
                                "worker shutdown deadline exceeded; unfinished recovery evidence is retained".into(),
                            )));
                        }
                    }
                } else {
                    tokio::select! {
                        result = workers.join_next() => result,
                        _ = stopping.wait_for(|stop| *stop) => {
                            stop_workers.send_replace(true);
                            shutdown_deadline = Some(tokio::time::Instant::now() + SLOT_SHUTDOWN_GRACE);
                            continue;
                        }
                    }
                };
                let Some(result) = result else { break; };
                if let Err(error) = result
                    .map_err(|error| DaemonError::Worker(error.to_string()))
                    .and_then(|result| result)
                {
                    tracing::error!(%error, "worker slot failed; stopping remaining slots through their shutdown path");
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    stop_workers.send_replace(true);
                    shutdown_deadline.get_or_insert_with(|| tokio::time::Instant::now() + SLOT_SHUTDOWN_GRACE);
                }
            }
            first_error.map_or(Ok(()), Err)
        })
        .await
}

async fn run_session(
    config: &SupervisorConfig,
    reconnect_delay: Duration,
    mut stopping: tokio::sync::watch::Receiver<bool>,
    stop_workers: Option<&tokio::sync::watch::Sender<bool>>,
) -> Result<(), DaemonError> {
    let identity = config.node.identity().await.map_err(Box::new)?;
    let mut runtime = runtime::WorkerRuntime::new(config, &identity.node_id)?;
    let result = async {
        loop {
            runtime.observe(config).await?;
            let step = tokio::select! {
                biased;
                _ = stopping.wait_for(|stop| *stop) => None,
                result = serve_once(config, &mut runtime) => Some(result?),
            };
            let Some(step) = step else {
                return tokio::time::timeout(SLOT_SHUTDOWN_GRACE, async {
                    runtime.shutdown(config).await?;
                    loop {
                        match serve_once(config, &mut runtime).await? {
                            DaemonExit::Finished => return Ok(()),
                            DaemonExit::Disconnected => {
                                tracing::info!(
                                    delay_seconds = reconnect_delay.as_secs(),
                                    "reconnecting to acknowledge shutdown receipts"
                                );
                                tokio::time::sleep(reconnect_delay).await;
                            }
                            DaemonExit::IncompleteEvidence { .. } => {
                                return Err(DaemonError::Worker(
                                    "server evidence is outside this runtime's recoverable interval".into(),
                                ));
                            }
                        }
                    }
                })
                .await
                .map_err(|_| DaemonError::Worker(
                    "worker shutdown deadline exceeded; durable state is retained for recovery".into(),
                ))?;
            };
            match step {
                DaemonExit::Finished => {
                    return Ok(());
                }
                DaemonExit::Disconnected => {
                    tracing::info!(
                        delay_seconds = reconnect_delay.as_secs(),
                        "reconnecting to Ackplane"
                    );
                    tokio::select! {
                        _ = stopping.wait_for(|stop| *stop) => {},
                        _ = tokio::time::sleep(reconnect_delay) => {},
                    }
                }
                DaemonExit::IncompleteEvidence {
                    local_acknowledged,
                    local_last_enqueued,
                    server_accepted,
                } => {
                    tracing::error!(
                        local_acknowledged,
                        local_last_enqueued,
                        server_accepted,
                        missing = server_accepted.saturating_sub(local_last_enqueued)
                            + local_acknowledged.saturating_sub(server_accepted),
                        "Ackplane and this outbox cannot reconcile retained supervisor evidence; \
                         refusing to resume. Investigate the durable state before restarting."
                    );
                    return Err(DaemonError::Worker(
                        "server evidence is outside this runtime's recoverable interval".into(),
                    ));
                }
            }
        }
    }
    .await;
    if let Err(error) = &result {
        if let Some(stop_workers) = stop_workers {
            stop_workers.send_replace(true);
        }
        tracing::warn!(%error, "session failed; stopping owned workers and releasing confirmed leases while retaining queued evidence");
        match tokio::time::timeout(SLOT_SHUTDOWN_GRACE, async {
            loop {
                runtime.shutdown(config).await?;
                if runtime.lease.is_none() {
                    return Ok::<(), DaemonError>(());
                }
                tokio::time::sleep(reconnect_delay).await;
            }
        })
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(cleanup_error)) => {
                tracing::error!(%cleanup_error, "failed slot cleanup stopped; recovery evidence is retained");
            }
            Err(_) => {
                tracing::error!(
                    "failed slot cleanup deadline exceeded; recovery evidence is retained"
                );
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SupervisorOutbox;
    use ackplane_client::companion::NodeClient;
    use ackplane_protocol::v1;
    use std::path::PathBuf;

    fn config() -> SupervisorConfig {
        SupervisorConfig {
            node: NodeClient::new(
                std::env::temp_dir().join("node-state"),
                "tenant-1".into(),
                "repository-1".into(),
            ),
            supervisor_id: "supervisor-1".to_string(),
            state_dir: PathBuf::from(".mindleak/supervisor"),
            heartbeat_interval: Duration::from_secs(30),
            workers: Default::default(),
        }
    }

    /// ADR-0116 decision 10, enforced by the declaration rather than by a
    /// check that could be forgotten. This build declares `Notify` and nothing
    /// else: a notification is complete once durably recorded, so it can
    /// honour that honestly, while every worker-driving capability is omitted
    /// because there is no worker to drive. An `Accepted` receipt for work
    /// nothing performed is therefore unreachable, not merely unlikely.
    #[test]
    fn a_daemon_without_a_worker_declares_only_what_it_can_honour() {
        let registration = registration(&config(), "node-1");
        let declared = &registration.capabilities.supported_directives;

        assert_eq!(declared, &vec![SupervisorDirectiveCapability::Notify]);
        for worker_driven in [
            SupervisorDirectiveCapability::Prompt,
            SupervisorDirectiveCapability::Assign,
            SupervisorDirectiveCapability::Steer,
            SupervisorDirectiveCapability::Pause,
            SupervisorDirectiveCapability::Resume,
            SupervisorDirectiveCapability::Drain,
            SupervisorDirectiveCapability::TerminateGracefully,
            SupervisorDirectiveCapability::TerminateForce,
        ] {
            assert!(
                !declared.contains(&worker_driven),
                "{worker_driven:?} needs a worker this build does not have; declaring it \
                 would produce an Accepted receipt for work that never happened"
            );
        }
        assert!(!registration.capabilities.supports_checkpoint);
        assert!(!registration.capabilities.supports_force_termination);
    }

    #[tokio::test]
    async fn an_unaccounted_worker_run_refuses_startup_before_reusing_its_workspace() {
        let root = tempfile::tempdir().unwrap();
        let mut config = config();
        config.state_dir = root.path().into();
        config.workers.insert(
            "first".into(),
            crate::WorkerCommand {
                command: "must-not-run".into(),
                args: vec!["{prompt}".into()],
                working_directory: root.path().into(),
                branch: "agents/first".into(),
            },
        );
        let marker = config.worker_run_path("first");
        std::fs::write(&marker, "unconfirmed previous run").unwrap();
        let (_stop, stopping) = tokio::sync::watch::channel(false);
        let error = run(&config, Duration::ZERO, stopping).await.unwrap_err();
        assert!(error.to_string().contains("unaccounted previous run"));
        assert!(
            marker.exists(),
            "refusal must preserve the previous run's evidence"
        );
    }

    // Canonicalization accepted regular files as workspaces, deferring the error
    // until a claimed task tried to spawn. Refuse them during startup instead.
    #[tokio::test]
    async fn startup_requires_worker_workspaces_to_be_directories() {
        for is_directory in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join("workspace");
            if is_directory {
                std::fs::create_dir(&workspace).unwrap();
            } else {
                std::fs::write(&workspace, "not a directory").unwrap();
            }
            let mut config = config();
            config.state_dir = root.path().join("state");
            config.workers.insert(
                "first".into(),
                crate::WorkerCommand {
                    command: "must-not-run".into(),
                    args: vec!["{prompt}".into()],
                    working_directory: workspace.clone(),
                    branch: "agents/first".into(),
                },
            );
            let (_stop, stopping) = tokio::sync::watch::channel(true);
            let result = run(&config, Duration::ZERO, stopping).await;
            if is_directory {
                result.unwrap();
            } else {
                let error = result.expect_err("a file is not a worker workspace");
                assert!(error.to_string().contains("first"));
                assert!(error.to_string().contains("not a directory"));
                assert_eq!(
                    std::fs::read_to_string(&workspace).unwrap(),
                    "not a directory"
                );
            }
            assert!(!config.worker_run_path("first").exists());
        }
    }

    /// The wire frame must carry the same declaration: a registration that is
    /// honest in memory and overstated on the wire is worse than one that is
    /// overstated in both, because only the wire one is believed.
    #[test]
    fn the_registration_frame_declares_the_same_capabilities() {
        let registration = registration(&config(), "node-1");
        let frame = registration_frame(&registration);

        let Some(v1::node_frame::Frame::SupervisorRegistration(wire)) = frame.frame else {
            panic!("expected a supervisor registration frame");
        };
        assert_eq!(
            wire.supported_directives,
            vec![v1::SupervisorDirectiveCapability::Notify as i32],
            "the wire declaration must match what the daemon can actually honour"
        );
        assert!(!wire.supports_checkpoint);
        assert_eq!(wire.node_id, "node-1");
        assert_eq!(wire.supervisor_id, "supervisor-1");
    }

    #[test]
    fn the_registration_is_valid_and_carries_the_configured_identity() {
        let registration = registration(&config(), "node-1");

        registration
            .validate()
            .expect("the daemon's own registration must be valid");
        assert_eq!(registration.identity.tenant_id, "tenant-1");
        assert_eq!(registration.identity.repository_id, "repository-1");
        assert_eq!(registration.supervisor_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn the_session_belongs_to_its_supervisor_and_validates() {
        let config = config();
        let session = session(&config, OffsetDateTime::now_utc()).expect("a session should build");

        session.validate().expect("the session must be valid");
        assert_eq!(session.supervisor_id, config.supervisor_id);
        assert!(session.session_id.starts_with(&config.supervisor_id));
    }

    /// Bug: registering, announcing a session, and submitting a directive
    /// receipt each propagated a transport failure as a fatal `DaemonError`
    /// (`.map_err(Box::new)?`), which `run`'s `serve_once(config).await?` has
    /// no way to catch -- ending the daemon permanently. The heartbeat a few
    /// lines below already treated the identical failure as an ordinary,
    /// retriable `Disconnected`. Nothing about those four connections
    /// differs, so a drop during registration should reconnect exactly like
    /// a drop during a heartbeat, not end the process.
    #[test]
    fn a_transport_failure_disconnects_rather_than_ending_the_daemon() {
        let ok: Result<(), ClientError> = Ok(());
        assert!(disconnected_on_error(ok).unwrap().is_none());

        let dropped: Result<(), ClientError> =
            Err(ClientError::InvalidEndpoint("unreachable".to_string()));
        assert!(matches!(
            disconnected_on_error(dropped),
            Ok(Some(DaemonExit::Disconnected))
        ));
    }

    fn queued_receipt() -> v1::DirectiveReceipt {
        v1::DirectiveReceipt {
            directive_id: "directive:stamp".to_string(),
            tenant_id: "tenant-1".to_string(),
            project_id: "project:stamp".to_string(),
            repository_id: "repository-1".to_string(),
            node_id: "node-1".to_string(),
            agent_session_id: "session:v1:agent-1".to_string(),
            status: v1::DirectiveReceiptStatus::Applied as i32,
            reason: v1::DirectiveReceiptReason::None as i32,
            occurred_at: "2026-08-30T00:00:00Z".to_string(),
            payload_digest: vec![3; 32],
            checkpoint_refs: Vec::new(),
            evidence_refs: Vec::new(),
            directive_sequence: 9,
            diagnostic: String::new(),
            outbox_sequence: None,
        }
    }

    fn test_outbox() -> SupervisorOutbox {
        let config = config();
        let session = session(&config, OffsetDateTime::now_utc())
            .expect("the test config describes a valid session");
        SupervisorOutbox::open_in_memory(registration(&config, "node-1"), session)
            .expect("an in-memory outbox opens")
    }

    fn stored_receipt(outbox: &SupervisorOutbox) -> (u64, v1::DirectiveReceipt) {
        let pending = outbox.pending(16).expect("pending frames are readable");
        match pending.as_slice() {
            [queued] => match &queued.frame.frame {
                Some(v1::node_frame::Frame::DirectiveReceipt(receipt)) => {
                    (queued.sequence, receipt.clone())
                }
                other => panic!("expected a queued directive receipt, got {other:?}"),
            },
            other => panic!("expected exactly one queued frame, got {other:?}"),
        }
    }

    /// Regression: the copy kept in the durable outbox and the copy put on the
    /// wire must carry the same outbox sequence.
    ///
    /// THE BUG THIS PREVENTS. ADR-0146 has the supervisor stamp its own outbox
    /// position onto each outbox-carried frame. The obvious implementation
    /// stamps only the copy being stored, because `enqueue_receipt` already
    /// took the receipt by reference and cloned it into the frame -- leaving
    /// the transmitted copy unstamped and, worse, leaving the two copies
    /// different. Ackplane keys a receipt's identity on a digest of the
    /// message, so the first transmission and any later resend of that same
    /// stored frame would not agree, and the resend would be recorded as a
    /// second, distinct receipt instead of recognised as the replay it is.
    /// The durable outbox would then manufacture duplicates rather than
    /// prevent loss, which is the opposite of why it exists.
    ///
    /// Fix: allocate the sequence once, stamp it before storing, and hand the
    /// stamped receipt back for transmission.
    #[test]
    fn a_queued_receipt_is_stamped_identically_in_the_outbox_and_on_the_wire() {
        let outbox = test_outbox();

        let (sequence, transmitted) =
            enqueue_receipt(&outbox, queued_receipt()).expect("the receipt queues");

        assert_eq!(sequence, 1);
        assert_eq!(transmitted.outbox_sequence, Some(1));

        let (stored_sequence, stored) = stored_receipt(&outbox);
        assert_eq!(stored_sequence, 1);
        assert_eq!(
            stored, transmitted,
            "the durable copy and the transmitted copy must be identical, or a resend \
             will not be recognised as a replay of the same decision"
        );
    }

    /// The stamp is the outbox's own position, not the server-issued
    /// directive's number. Confusing the two is exactly the mistake ADR-0146
    /// was written to correct, and they are both `u64` so nothing but a test
    /// notices when they are swapped.
    #[test]
    fn the_stamp_is_the_outbox_position_not_the_directive_sequence() {
        let outbox = test_outbox();
        let receipt = queued_receipt();
        assert_eq!(
            receipt.directive_sequence, 9,
            "the fixture must differ from the outbox position for this test to mean anything"
        );

        let (_, transmitted) = enqueue_receipt(&outbox, receipt).expect("the receipt queues");

        assert_eq!(transmitted.outbox_sequence, Some(1));
        assert_eq!(transmitted.directive_sequence, 9);
    }

    /// Each enqueue takes the next position, so the server can tell how far
    /// this supervisor has actually got rather than only that it sent
    /// something.
    #[test]
    fn each_queued_receipt_takes_the_next_outbox_position() {
        let outbox = test_outbox();

        let (first, _) = enqueue_receipt(&outbox, queued_receipt()).expect("the first queues");
        outbox
            .acknowledge_through(first)
            .expect("the first is acknowledged");
        let mut second_receipt = queued_receipt();
        second_receipt.directive_id = "directive:stamp-2".to_string();
        let (second, transmitted) =
            enqueue_receipt(&outbox, second_receipt).expect("the second queues");

        assert_eq!((first, second), (1, 2));
        assert_eq!(transmitted.outbox_sequence, Some(2));
    }
}
