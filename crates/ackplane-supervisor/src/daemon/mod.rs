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
use ackplane_protocol::supervisor::{
    SupervisorCapabilities, SupervisorDirectiveCapability, SupervisorIdentity,
    SupervisorOutboxDurability, SupervisorRegistration, SupervisorRuntime, SupervisorSession,
    SupervisorWorkerState,
};
use time::OffsetDateTime;

mod delivery;
mod frames;

use delivery::enqueue_receipt;
pub use delivery::resend_pending;
use frames::{heartbeat_frame, registration_frame, session_frame};

use crate::{
    config::{SignerSource, SupervisorConfig},
    reconcile::{reconcile, Reconciliation},
    InboxError, OutboxError, SupervisorInbox, SupervisorOutbox,
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
        .exchange_supervisor_frame(registration_frame(&registration))
        .await
    {
        Ok(receipt) => receipt,
        Err(error) => {
            tracing::info!(%error, "the supervisor connection closed");
            return Ok(DaemonExit::Disconnected);
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
                server_accepted,
            } => {
                tracing::error!(
                    local_acknowledged,
                    server_accepted,
                    "Ackplane has durably accepted supervisor frames this outbox cannot \
                     account for; stopping rather than resuming on incomplete evidence"
                );
                return Ok(DaemonExit::IncompleteEvidence {
                    local_acknowledged,
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

    if let Some(exit) = resend_pending(&outbox, &mut connection).await? {
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
            // Durable before transmitted. The receipt is already recorded in
            // the inbox, but the inbox records what this supervisor *decided*;
            // the outbox records what it still owes the server. Without this,
            // a receipt computed and then lost to a dropped connection depends
            // entirely on the server redelivering the directive to be sent
            // again -- true today, but a guarantee held by the other side of a
            // connection that had just failed.
            let (sequence, receipt) = enqueue_receipt(&outbox, receipt)?;
            if let Some(exit) =
                disconnected_on_error(connection.submit_directive_receipt(receipt).await)
            {
                return Ok(exit);
            }
            // Acknowledged only once the server's own frame receipt confirms
            // it, so an unconfirmed receipt survives to be resent.
            outbox.acknowledge_through(sequence)?;
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
    use ackplane_protocol::v1;
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
        SupervisorOutbox::open_in_memory(registration(&config), session)
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
