//! Directive validation and receipt decisions for one local supervisor inbox.
//!
//! ADR-0116 requires a local supervisor to retain receipt and sequencing state
//! across reconnects. This crate deliberately does not open a network listener,
//! launch a worker, or execute a directive. A future transport adapter feeds
//! [`SupervisorInbox::receive`] the directives it received over NodeSync.

use std::{fs, path::Path, time::Duration};

use crate::storage::{ensure_inbox_identity, load_receipt, next_sequence, store_receipt, SCHEMA};

use ackplane_protocol::{
    supervisor::{
        SupervisorDirectiveCapability, SupervisorError, SupervisorIdentity, SupervisorRegistration,
        SupervisorSession,
    },
    v1::{self, agent_directive},
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use thiserror::Error;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

/// A path-owned local inbox for one supervisor and one active worker session.
pub struct SupervisorInbox {
    conn: Connection,
    registration: SupervisorRegistration,
    session: SupervisorSession,
}

impl SupervisorInbox {
    /// Open or create the local inbox at `path` and bind it to one identity/session pair.
    pub fn open(
        path: impl AsRef<Path>,
        registration: SupervisorRegistration,
        session: SupervisorSession,
    ) -> Result<Self, InboxError> {
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::from_connection(conn, registration, session)
    }

    /// Build an ephemeral inbox for focused tests and tooling.
    pub fn open_in_memory(
        registration: SupervisorRegistration,
        session: SupervisorSession,
    ) -> Result<Self, InboxError> {
        Self::from_connection(Connection::open_in_memory()?, registration, session)
    }

    fn from_connection(
        conn: Connection,
        registration: SupervisorRegistration,
        session: SupervisorSession,
    ) -> Result<Self, InboxError> {
        registration.validate()?;
        session.validate()?;
        if session.supervisor_id != registration.supervisor_id {
            return Err(InboxError::SessionSupervisorMismatch);
        }

        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        ensure_inbox_identity(
            &conn,
            &registration.identity,
            &registration.supervisor_id,
            &session,
        )?;

        Ok(Self {
            conn,
            registration,
            session,
        })
    }

    /// Receive one directive and return the durable terminal receipt.
    ///
    /// This method accepts a directive only after validating that it targets this
    /// inbox, names a capability this supervisor declared, is not expired, and
    /// continues the local sequence. A duplicate directive id/digest returns its
    /// original receipt; a changed digest for the same id is a refusal.
    pub fn receive(
        &self,
        directive: &v1::AgentDirective,
        now: OffsetDateTime,
    ) -> Result<v1::DirectiveReceipt, InboxError> {
        self.validate_target(directive)?;
        require_non_empty("directive_id", &directive.directive_id)?;
        if directive.payload_digest.is_empty() {
            return Err(InboxError::MissingPayloadDigest {
                directive_id: directive.directive_id.clone(),
            });
        }

        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if let Some(stored) = load_receipt(&transaction, &directive.directive_id)? {
            if stored.payload_digest == directive.payload_digest {
                return Ok(stored.into_receipt());
            }
            return Err(InboxError::PayloadDigestMismatch {
                directive_id: directive.directive_id.clone(),
            });
        }

        let (capability, required_capability) = required_capability(directive)?;
        if directive.required_capability != required_capability {
            return Err(InboxError::RequiredCapabilityMismatch {
                directive_id: directive.directive_id.clone(),
                expected: required_capability.to_string(),
                actual: directive.required_capability.clone(),
            });
        }

        let sequence =
            i64::try_from(directive.sequence).map_err(|_| InboxError::SequenceOutOfRange {
                directive_id: directive.directive_id.clone(),
            })?;
        if sequence <= 0 {
            return Err(InboxError::SequenceMustBePositive {
                directive_id: directive.directive_id.clone(),
            });
        }
        let expected = next_sequence(&transaction)?;
        if sequence != expected {
            return Err(InboxError::SequenceGap {
                directive_id: directive.directive_id.clone(),
                expected: expected as u64,
                received: directive.sequence,
            });
        }

        let expires_at = OffsetDateTime::parse(&directive.expires_at, &Rfc3339).map_err(|_| {
            InboxError::InvalidExpiry {
                directive_id: directive.directive_id.clone(),
            }
        })?;
        let occurred_at = now
            .format(&Rfc3339)
            .map_err(|_| InboxError::ClockOutOfRange)?;
        let (status, reason) = if expires_at <= now {
            (
                v1::DirectiveReceiptStatus::Expired,
                v1::DirectiveReceiptReason::Expired,
            )
        } else if !self
            .registration
            .capabilities
            .supported_directives
            .contains(&capability)
        {
            (
                v1::DirectiveReceiptStatus::Refused,
                v1::DirectiveReceiptReason::CapabilityMissing,
            )
        } else {
            (
                v1::DirectiveReceiptStatus::Accepted,
                v1::DirectiveReceiptReason::None,
            )
        };

        let receipt = receipt_for(
            directive,
            &self.registration.identity,
            &self.session,
            status,
            reason,
            occurred_at,
        );
        store_receipt(&transaction, &receipt)?;
        transaction.commit()?;
        Ok(receipt)
    }

    fn validate_target(&self, directive: &v1::AgentDirective) -> Result<(), InboxError> {
        let identity = &self.registration.identity;
        if directive.tenant_id != identity.tenant_id
            || directive.repository_id != identity.repository_id
            || directive.target_node_id != identity.node_id
            || directive.target_agent_session_id != self.session.session_id
        {
            return Err(InboxError::TargetMismatch {
                directive_id: directive.directive_id.clone(),
            });
        }
        Ok(())
    }
}

/// Durable-inbox errors are explicit so a transport adapter can report a typed receipt or refusal.
#[derive(Debug, Error)]
pub enum InboxError {
    #[error("invalid supervisor declaration: {0}")]
    Supervisor(#[from] SupervisorError),
    #[error("supervisor session does not belong to the configured supervisor")]
    SessionSupervisorMismatch,
    #[error("directive inbox I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("directive inbox database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("directive {directive_id:?} does not target this supervisor session")]
    TargetMismatch { directive_id: String },
    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },
    #[error("directive {directive_id:?} has no payload digest")]
    MissingPayloadDigest { directive_id: String },
    #[error("directive {directive_id:?} has an unsupported kind or payload")]
    UnsupportedDirective { directive_id: String },
    #[error("directive {directive_id:?} required capability mismatch: expected {expected:?}, got {actual:?}")]
    RequiredCapabilityMismatch {
        directive_id: String,
        expected: String,
        actual: String,
    },
    #[error("directive {directive_id:?} has an invalid expiry timestamp")]
    InvalidExpiry { directive_id: String },
    #[error("directive {directive_id:?} sequence must be positive")]
    SequenceMustBePositive { directive_id: String },
    #[error("directive {directive_id:?} sequence exceeds the local inbox range")]
    SequenceOutOfRange { directive_id: String },
    #[error("directive {directive_id:?} is out of sequence: expected {expected}, got {received}")]
    SequenceGap {
        directive_id: String,
        expected: u64,
        received: u64,
    },
    #[error("directive {directive_id:?} replay changed its payload digest")]
    PayloadDigestMismatch { directive_id: String },
    #[error("the current time cannot be formatted as RFC3339")]
    ClockOutOfRange,
    #[error("the durable inbox is already bound to another supervisor identity or session")]
    InboxIdentityMismatch,
}

fn required_capability(
    directive: &v1::AgentDirective,
) -> Result<(SupervisorDirectiveCapability, &'static str), InboxError> {
    let kind = v1::DirectiveKind::try_from(directive.kind).map_err(|_| {
        InboxError::UnsupportedDirective {
            directive_id: directive.directive_id.clone(),
        }
    })?;
    let payload = directive
        .payload
        .as_ref()
        .ok_or_else(|| InboxError::UnsupportedDirective {
            directive_id: directive.directive_id.clone(),
        })?;

    let capability = match (kind, payload) {
        (v1::DirectiveKind::Notify, agent_directive::Payload::Notify(_)) => {
            (SupervisorDirectiveCapability::Notify, "notify.v1")
        }
        (v1::DirectiveKind::Prompt, agent_directive::Payload::Prompt(_)) => {
            (SupervisorDirectiveCapability::Prompt, "prompt.v1")
        }
        (v1::DirectiveKind::Assign, agent_directive::Payload::Assign(_)) => {
            (SupervisorDirectiveCapability::Assign, "assign.v1")
        }
        (v1::DirectiveKind::Steer, agent_directive::Payload::Steer(_)) => {
            (SupervisorDirectiveCapability::Steer, "steer.v1")
        }
        (v1::DirectiveKind::Pause, agent_directive::Payload::Pause(_)) => {
            (SupervisorDirectiveCapability::Pause, "pause.v1")
        }
        (v1::DirectiveKind::Resume, agent_directive::Payload::Resume(_)) => {
            (SupervisorDirectiveCapability::Resume, "resume.v1")
        }
        (v1::DirectiveKind::Drain, agent_directive::Payload::Drain(_)) => {
            (SupervisorDirectiveCapability::Drain, "drain.v1")
        }
        (v1::DirectiveKind::Terminate, agent_directive::Payload::Terminate(termination)) => {
            match v1::TerminationMode::try_from(termination.mode).ok() {
                Some(v1::TerminationMode::Graceful) => (
                    SupervisorDirectiveCapability::TerminateGracefully,
                    "terminate.graceful.v1",
                ),
                Some(v1::TerminationMode::ForceAfterDeadline | v1::TerminationMode::Force) => (
                    SupervisorDirectiveCapability::TerminateForce,
                    "terminate.force.v1",
                ),
                _ => {
                    return Err(InboxError::UnsupportedDirective {
                        directive_id: directive.directive_id.clone(),
                    });
                }
            }
        }
        _ => {
            return Err(InboxError::UnsupportedDirective {
                directive_id: directive.directive_id.clone(),
            });
        }
    };
    Ok(capability)
}

fn receipt_for(
    directive: &v1::AgentDirective,
    identity: &SupervisorIdentity,
    session: &SupervisorSession,
    status: v1::DirectiveReceiptStatus,
    reason: v1::DirectiveReceiptReason,
    occurred_at: String,
) -> v1::DirectiveReceipt {
    v1::DirectiveReceipt {
        directive_id: directive.directive_id.clone(),
        tenant_id: identity.tenant_id.clone(),
        project_id: directive.project_id.clone(),
        repository_id: identity.repository_id.clone(),
        node_id: identity.node_id.clone(),
        agent_session_id: session.session_id.clone(),
        status: status as i32,
        reason: reason as i32,
        occurred_at,
        payload_digest: directive.payload_digest.clone(),
        checkpoint_refs: Vec::new(),
        evidence_refs: Vec::new(),
        directive_sequence: directive.sequence,
        diagnostic: String::new(),
    }
}

fn require_non_empty(field: &'static str, value: &str) -> Result<(), InboxError> {
    if value.trim().is_empty() {
        return Err(InboxError::EmptyField { field });
    }
    Ok(())
}
