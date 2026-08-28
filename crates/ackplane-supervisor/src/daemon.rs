//! The supervisor daemon's run loop (ADR-0116: "an enrolled supervisor is the
//! only Industrial runtime endpoint").
//!
//! This assembles slices 1-4 into something an operator can actually run:
//! connect and authenticate, register, open a session, receive directives and
//! durably receipt them, heartbeat, and on reconnect reconcile position rather
//! than assuming a clean resume.
//!
//! # What this daemon deliberately cannot do
//!
//! No [`WorkerAdapter`](crate::WorkerAdapter) is wired in, so it cannot drive a
//! worker process — and it says so in its own declaration rather than by a
//! special case at delivery time.
//!
//! It declares exactly one capability: `Notify`. That is not a placeholder. A
//! `NotifyDirective` carries a message *to the supervisor*, so receiving it and
//! durably recording it **is** the whole action; there is no worker step being
//! skipped, and an `Accepted` receipt for one is truthful. Every other
//! capability — `Prompt`, `Assign`, `Steer`, `Pause`, `Resume`, `Drain`,
//! `TerminateGracefully`, `TerminateForce` — needs a worker to act on, so this
//! build declares none of them.
//!
//! The consequence is what ADR-0116 decision 10 asks for, enforced by the
//! declaration rather than by remembering to check: a directive this supervisor
//! cannot execute is refused by the server before it is ever enqueued, and if
//! one is delivered anyway the existing [`SupervisorInbox`](crate::SupervisorInbox)
//! answers it with a durable `Refused` / `CapabilityMissing` receipt. An
//! `Accepted` receipt for work nothing performed is unreachable, not merely
//! unlikely.
//!
//! Declaring a capability this build cannot honour is the one mistake worth
//! preventing structurally: it would produce a receipt saying work happened
//! when it did not, and every layer above believes the receipt.

use std::time::Duration;

use ackplane_client::{
    auth::{ClaimSigner, CredentialFacilitySigner, SeedSigner},
    node_sync::NodeSyncConnection,
    ClientError,
};
use ackplane_protocol::{
    supervisor::{
        SupervisorCapabilities, SupervisorDirectiveCapability, SupervisorIdentity,
        SupervisorOutboxDurability, SupervisorRegistration, SupervisorRuntime, SupervisorSession,
        SupervisorWorkerState,
    },
    v1,
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::{
    config::{SignerSource, SupervisorConfig},
    reconcile, InboxError, OutboxError, Reconciliation, SupervisorInbox, SupervisorOutbox,
};

/// How the daemon stopped, so a caller can distinguish an orderly shutdown
/// from a condition that needs a person.
#[derive(Debug)]
pub enum DaemonExit {
    /// The connection closed and the daemon stopped cleanly.
    Disconnected,
    /// Durable local state cannot account for what the server holds. Reported
    /// rather than resumed (ADR-0116 decision 3); an operator decides.
    IncompleteEvidence {
        local_acknowledged: u64,
        server_accepted: u64,
    },
}

/// Everything that can stop the daemon before it is running.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("the supervisor's signing key could not be loaded: {0}")]
    Signer(String),
    #[error("connecting to Ackplane failed: {0}")]
    Connect(#[from] Box<ClientError>),
    #[error("the durable inbox could not be opened: {0}")]
    Inbox(#[from] InboxError),
    #[error("the durable outbox could not be opened: {0}")]
    Outbox(#[from] OutboxError),
    #[error("the supervisor clock is outside the representable range")]
    Clock,
}

/// The runtime this daemon reports itself as. It supervises a worker on the
/// machine it runs on; it does not claim a container or cloud runtime it has
/// no way to verify.
const RUNTIME: SupervisorRuntime = SupervisorRuntime::LocalMachine;

/// Build the signer this configuration selected.
pub fn signer(config: &SupervisorConfig) -> Result<Box<dyn ClaimSigner>, DaemonError> {
    match &config.signer_source {
        SignerSource::Seed(seed) => Ok(Box::new(SeedSigner::new(
            config.signing_key_id.clone(),
            config.node_id.clone(),
            seed.as_ref(),
        ))),
        SignerSource::CredentialFacility { service, account } => CredentialFacilitySigner::load(
            config.signing_key_id.clone(),
            config.node_id.clone(),
            service,
            account,
        )
        .map(|signer| Box::new(signer) as Box<dyn ClaimSigner>)
        .map_err(|error| DaemonError::Signer(error.to_string())),
    }
}

/// This supervisor's registration: an honest declaration of what it can do.
///
/// `Notify` only. A notification is complete once durably recorded, so this
/// build can genuinely honour it. Every worker-driving capability is omitted,
/// which makes the server refuse to enqueue such a directive in the first
/// place rather than this daemon having to refuse it after delivery.
pub fn registration(config: &SupervisorConfig) -> SupervisorRegistration {
    SupervisorRegistration {
        supervisor_id: config.supervisor_id.clone(),
        identity: SupervisorIdentity {
            tenant_id: config.tenant_id.clone(),
            repository_id: config.repository_id.clone(),
            node_id: config.node_id.clone(),
        },
        supervisor_version: env!("CARGO_PKG_VERSION").to_string(),
        protocol_version: "v1".to_string(),
        capabilities: SupervisorCapabilities {
            supported_directives: vec![SupervisorDirectiveCapability::Notify],
            // Without a worker there is nothing to checkpoint or terminate.
            // Claiming either would be a promise this build cannot keep.
            supports_checkpoint: false,
            supports_force_termination: false,
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

fn registration_frame(registration: &SupervisorRegistration) -> v1::NodeFrame {
    v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::SupervisorRegistration(
            v1::SupervisorRegistration {
                supervisor_id: registration.supervisor_id.clone(),
                node_id: registration.identity.node_id.clone(),
                supervisor_version: registration.supervisor_version.clone(),
                protocol_version: registration.protocol_version.clone(),
                supported_directives: registration
                    .capabilities
                    .supported_directives
                    .iter()
                    .map(|capability| wire_capability(*capability) as i32)
                    .collect(),
                supports_checkpoint: registration.capabilities.supports_checkpoint,
                supports_force_termination: registration.capabilities.supports_force_termination,
                outbox_durability: v1::SupervisorOutboxDurability::Persistent as i32,
                recoverable_outbox: registration.capabilities.recoverable_outbox,
            },
        )),
    }
}

/// Map one declared capability onto its wire value.
///
/// Exhaustive rather than a catch-all: a capability added to the protocol must
/// be considered here deliberately, not silently dropped from a registration
/// that would then under-declare what this supervisor can do.
fn wire_capability(capability: SupervisorDirectiveCapability) -> v1::SupervisorDirectiveCapability {
    match capability {
        SupervisorDirectiveCapability::Notify => v1::SupervisorDirectiveCapability::Notify,
        SupervisorDirectiveCapability::Prompt => v1::SupervisorDirectiveCapability::Prompt,
        SupervisorDirectiveCapability::Assign => v1::SupervisorDirectiveCapability::Assign,
        SupervisorDirectiveCapability::Steer => v1::SupervisorDirectiveCapability::Steer,
        SupervisorDirectiveCapability::Pause => v1::SupervisorDirectiveCapability::Pause,
        SupervisorDirectiveCapability::Resume => v1::SupervisorDirectiveCapability::Resume,
        SupervisorDirectiveCapability::Drain => v1::SupervisorDirectiveCapability::Drain,
        SupervisorDirectiveCapability::TerminateGracefully => {
            v1::SupervisorDirectiveCapability::TerminateGracefully
        }
        SupervisorDirectiveCapability::TerminateForce => {
            v1::SupervisorDirectiveCapability::TerminateForce
        }
    }
}

fn session_frame(
    session: &SupervisorSession,
    started_at: OffsetDateTime,
) -> Result<v1::NodeFrame, DaemonError> {
    Ok(v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::SupervisorSession(
            v1::SupervisorSession {
                supervisor_id: session.supervisor_id.clone(),
                session_id: session.session_id.clone(),
                worker_id: session.worker_id.clone(),
                runtime: v1::SupervisorRuntime::LocalMachine as i32,
                started_at: started_at
                    .format(&Rfc3339)
                    .map_err(|_| DaemonError::Clock)?,
                state: v1::SupervisorWorkerState::Started as i32,
            },
        )),
    })
}

fn heartbeat_frame(supervisor_id: &str) -> v1::NodeFrame {
    v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::SupervisorHeartbeat(
            v1::SupervisorHeartbeat {
                supervisor_id: supervisor_id.to_string(),
            },
        )),
    }
}

/// Open one connection, announce this supervisor, and serve directives until
/// the connection closes or local evidence stops adding up.
///
/// One connection per call deliberately: reconnect policy (how long to wait,
/// how many times, whether to give up) is an operator concern, and burying it
/// in the loop would make it untestable and unconfigurable. [`run`] supplies a
/// simple policy over this.
pub async fn serve_once(config: &SupervisorConfig) -> Result<DaemonExit, DaemonError> {
    let started_at = OffsetDateTime::now_utc();
    let registration = registration(config);
    let session = session(config, started_at)?;

    let inbox = SupervisorInbox::open(config.inbox_path(), registration.clone(), session.clone())?;
    let outbox =
        SupervisorOutbox::open(config.outbox_path(), registration.clone(), session.clone())?;
    let positions = outbox.positions()?;

    let signer = signer(config)?;
    let mut connection = NodeSyncConnection::open(
        &config.endpoint,
        signer.as_ref(),
        &config.tenant_id,
        &config.repository_id,
        vec!["synchronize".to_string()],
        positions.acknowledged,
    )
    .await
    .map_err(Box::new)?;

    // ADR-0116 decision 7: decide whether this is a clean resume before
    // sending anything, not after.
    match reconcile(positions, connection.accepted_position()) {
        Reconciliation::IncompleteEvidence {
            local_acknowledged,
            server_accepted,
        } => {
            return Ok(DaemonExit::IncompleteEvidence {
                local_acknowledged,
                server_accepted,
            });
        }
        Reconciliation::UpToDate { .. } | Reconciliation::Resend { .. } => {}
    }

    if let Some(exit) = disconnected_on_error(
        connection
            .exchange_supervisor_frame(registration_frame(&registration))
            .await,
    ) {
        return Ok(exit);
    }
    if let Some(exit) = disconnected_on_error(
        connection
            .exchange_supervisor_frame(session_frame(&session, started_at)?)
            .await,
    ) {
        return Ok(exit);
    }

    loop {
        // The session frame delivers this session's pending directives ahead
        // of its own receipt, so they are already in hand here.
        while let Some(directive) = connection.next_directive() {
            let receipt = match inbox.receive(&directive, OffsetDateTime::now_utc()) {
                Ok(receipt) => receipt,
                Err(error) => {
                    // A directive this inbox refuses outright (wrong target, a
                    // changed digest for a known id) has no receipt to return.
                    // It is reported rather than dropped quietly.
                    tracing::warn!(
                        directive_id = %directive.directive_id,
                        %error,
                        "the supervisor refused a directive before receipting it"
                    );
                    continue;
                }
            };
            tracing::info!(
                directive_id = %receipt.directive_id,
                status = receipt.status,
                reason = receipt.reason,
                "receipted a directive"
            );
            if let Some(exit) =
                disconnected_on_error(connection.submit_directive_receipt(receipt).await)
            {
                return Ok(exit);
            }
        }

        tokio::time::sleep(config.heartbeat_interval).await;
        if let Some(exit) = disconnected_on_error(
            connection
                .exchange_supervisor_frame(heartbeat_frame(&config.supervisor_id))
                .await,
        ) {
            return Ok(exit);
        }
        // Re-announcing the session is what asks for newly issued directives:
        // delivery is bound to the session frame, so this is the poll.
        if let Some(exit) = disconnected_on_error(
            connection
                .exchange_supervisor_frame(session_frame(&session, started_at)?)
                .await,
        ) {
            return Ok(exit);
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
fn disconnected_on_error<T>(result: Result<T, ClientError>) -> Option<DaemonExit> {
    match result {
        Ok(_) => None,
        Err(error) => {
            tracing::info!(%error, "the supervisor connection closed");
            Some(DaemonExit::Disconnected)
        }
    }
}

/// Serve, reconnecting on a dropped connection until evidence stops adding up.
///
/// `IncompleteEvidence` deliberately stops the daemon instead of retrying:
/// reconnecting cannot restore a durable record that is already gone, so a
/// retry loop would turn a reportable condition into an invisible one.
pub async fn run(config: &SupervisorConfig, reconnect_delay: Duration) -> Result<(), DaemonError> {
    loop {
        match serve_once(config).await? {
            DaemonExit::Disconnected => {
                tracing::info!(
                    delay_seconds = reconnect_delay.as_secs(),
                    "reconnecting to Ackplane"
                );
                tokio::time::sleep(reconnect_delay).await;
            }
            DaemonExit::IncompleteEvidence {
                local_acknowledged,
                server_accepted,
            } => {
                tracing::error!(
                    local_acknowledged,
                    server_accepted,
                    missing = server_accepted.saturating_sub(local_acknowledged),
                    "Ackplane holds supervisor evidence this node cannot account for; \
                     refusing to resume. Investigate the durable state before restarting."
                );
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SignerSource;
    use std::path::PathBuf;

    fn config() -> SupervisorConfig {
        SupervisorConfig {
            endpoint: "http://127.0.0.1:8443".to_string(),
            tenant_id: "tenant-1".to_string(),
            repository_id: "repository-1".to_string(),
            node_id: "node-1".to_string(),
            signing_key_id: "signing-key-1".to_string(),
            supervisor_id: "supervisor-1".to_string(),
            signer_source: SignerSource::Seed(Box::new([7; 32])),
            state_dir: PathBuf::from(".mindleak/supervisor"),
            heartbeat_interval: Duration::from_secs(30),
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
        let registration = registration(&config());
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

    /// The wire frame must carry the same declaration: a registration that is
    /// honest in memory and overstated on the wire is worse than one that is
    /// overstated in both, because only the wire one is believed.
    #[test]
    fn the_registration_frame_declares_the_same_capabilities() {
        let registration = registration(&config());
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
        let registration = registration(&config());

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
        assert!(disconnected_on_error(ok).is_none());

        let dropped: Result<(), ClientError> =
            Err(ClientError::InvalidEndpoint("unreachable".to_string()));
        assert!(matches!(
            disconnected_on_error(dropped),
            Some(DaemonExit::Disconnected)
        ));
    }
}
