//! Storage-independent enrolled-supervisor contracts for Industrial agents.
//!
//! ADR-0116 requires one narrow runtime endpoint across laptops, cloud workers,
//! pipelines, and long-running services. This module records what a supervisor
//! truthfully supports; a future store and transport own registration,
//! authorization, delivery, and worker execution.

use std::collections::HashSet;

use prost::Message;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::v1::{self, agent_directive};

/// An enrolled supervisor's stable identity within one tenant and repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorIdentity {
    pub tenant_id: String,
    pub repository_id: String,
    pub node_id: String,
}

impl SupervisorIdentity {
    pub fn validate(&self) -> Result<(), SupervisorError> {
        for (field, value) in [
            ("tenant_id", self.tenant_id.as_str()),
            ("repository_id", self.repository_id.as_str()),
            ("node_id", self.node_id.as_str()),
        ] {
            require_non_empty(field, value)?;
        }
        Ok(())
    }
}

/// A versioned declaration of the capabilities one enrolled supervisor can enforce.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorRegistration {
    pub supervisor_id: String,
    pub identity: SupervisorIdentity,
    pub supervisor_version: String,
    pub protocol_version: String,
    pub capabilities: SupervisorCapabilities,
}

impl SupervisorRegistration {
    /// Reject impossible or misleading declarations before a future store accepts them.
    pub fn validate(&self) -> Result<(), SupervisorError> {
        require_non_empty("supervisor_id", &self.supervisor_id)?;
        require_non_empty("supervisor_version", &self.supervisor_version)?;
        require_non_empty("protocol_version", &self.protocol_version)?;
        self.identity.validate()?;
        self.capabilities.validate()
    }
}

/// The bounded controls and delivery guarantees an enrolled supervisor advertises.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorCapabilities {
    pub supported_directives: Vec<SupervisorDirectiveCapability>,
    pub supports_checkpoint: bool,
    pub supports_force_termination: bool,
    pub outbox_durability: SupervisorOutboxDurability,
    pub recoverable_outbox: bool,
}

impl SupervisorCapabilities {
    pub fn validate(&self) -> Result<(), SupervisorError> {
        if self.supported_directives.is_empty() {
            return Err(SupervisorError::NoDirectiveCapabilities);
        }

        let mut directives = HashSet::new();
        for directive in &self.supported_directives {
            if !directives.insert(*directive) {
                return Err(SupervisorError::DuplicateDirectiveCapability {
                    capability: *directive,
                });
            }
        }

        let force_termination_advertised =
            directives.contains(&SupervisorDirectiveCapability::TerminateForce);
        if self.supports_force_termination != force_termination_advertised {
            return Err(SupervisorError::ForceTerminationMismatch {
                supports_force_termination: self.supports_force_termination,
                force_termination_advertised,
            });
        }

        if self.outbox_durability == SupervisorOutboxDurability::Ephemeral
            && self.recoverable_outbox
        {
            return Err(SupervisorError::EphemeralOutboxCannotBeRecoverable);
        }

        Ok(())
    }
}

/// A closed control vocabulary that a supervisor can advertise as locally enforceable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorDirectiveCapability {
    Notify,
    Prompt,
    Assign,
    Steer,
    Pause,
    Resume,
    Drain,
    TerminateGracefully,
    TerminateForce,
}

/// The precise supervisor capability a closed wire directive requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectiveRequirement {
    pub capability: SupervisorDirectiveCapability,
    pub required_capability: &'static str,
}

/// Maps a typed `AgentDirective` payload to the capability a supervisor must
/// have advertised. Unknown, unspecified, and mismatched kind/payload pairs
/// deliberately produce no requirement rather than falling through to a
/// permissive default.
pub fn directive_requirement(directive: &v1::AgentDirective) -> Option<DirectiveRequirement> {
    let kind = v1::DirectiveKind::try_from(directive.kind).ok()?;
    let payload = directive.payload.as_ref()?;
    let (capability, required_capability) = match (kind, payload) {
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
            match v1::TerminationMode::try_from(termination.mode).ok()? {
                v1::TerminationMode::Graceful => (
                    SupervisorDirectiveCapability::TerminateGracefully,
                    "terminate.graceful.v1",
                ),
                v1::TerminationMode::ForceAfterDeadline | v1::TerminationMode::Force => (
                    SupervisorDirectiveCapability::TerminateForce,
                    "terminate.force.v1",
                ),
                v1::TerminationMode::Unspecified => return None,
            }
        }
        _ => return None,
    };
    Some(DirectiveRequirement {
        capability,
        required_capability,
    })
}

/// Returns the domain-separated digest of a directive's typed payload.
///
/// The digest intentionally includes the declared kind as well as the encoded
/// payload. Empty protobuf messages such as `Assign` and `Resume` otherwise
/// have the same byte representation, which would let a receipt bind the
/// wrong closed action to the same digest.
pub fn directive_payload_digest(directive: &v1::AgentDirective) -> Option<Vec<u8>> {
    directive_requirement(directive)?;
    let payload = match directive.payload.as_ref()? {
        agent_directive::Payload::Notify(value) => value.encode_to_vec(),
        agent_directive::Payload::Prompt(value) => value.encode_to_vec(),
        agent_directive::Payload::Assign(value) => value.encode_to_vec(),
        agent_directive::Payload::Steer(value) => value.encode_to_vec(),
        agent_directive::Payload::Pause(value) => value.encode_to_vec(),
        agent_directive::Payload::Resume(value) => value.encode_to_vec(),
        agent_directive::Payload::Drain(value) => value.encode_to_vec(),
        agent_directive::Payload::Terminate(value) => value.encode_to_vec(),
    };
    let mut hasher = Sha256::new();
    hasher.update(b"mindleak.ackplane.v1.agent_directive.payload\0");
    hasher.update(directive.kind.to_be_bytes());
    hasher.update(payload);
    Some(hasher.finalize().to_vec())
}

/// Whether a supervisor can honestly recover its local outbox after process loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorOutboxDurability {
    Persistent,
    Ephemeral,
}

/// One supervisor-owned worker session, distinct from the supervisor's stable identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorSession {
    pub session_id: String,
    pub supervisor_id: String,
    pub worker_id: String,
    pub runtime: SupervisorRuntime,
    pub started_at: i64,
    pub state: SupervisorWorkerState,
}

impl SupervisorSession {
    pub fn validate(&self) -> Result<(), SupervisorError> {
        for (field, value) in [
            ("session_id", self.session_id.as_str()),
            ("supervisor_id", self.supervisor_id.as_str()),
            ("worker_id", self.worker_id.as_str()),
        ] {
            require_non_empty(field, value)?;
        }
        Ok(())
    }
}

/// The local runtime class a supervisor adapts, not an authority designation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorRuntime {
    LocalMachine,
    CloudWorker,
    Pipeline,
    Service,
}

/// The last supervisor-observed state of a worker it owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorWorkerState {
    Started,
    Checkpointed,
    Paused,
    Draining,
    Terminated,
    Failed,
    Disconnected,
    Reconnected,
    Completed,
}

/// An attributed observation of a supervisor-owned worker lifecycle transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorLifecycleReceipt {
    pub supervisor_id: String,
    pub session_id: String,
    pub worker_id: String,
    pub occurred_at: i64,
    pub state: SupervisorWorkerState,
    pub reason: Option<SupervisorLifecycleReason>,
}

impl SupervisorLifecycleReceipt {
    pub fn validate(&self) -> Result<(), SupervisorError> {
        for (field, value) in [
            ("supervisor_id", self.supervisor_id.as_str()),
            ("session_id", self.session_id.as_str()),
            ("worker_id", self.worker_id.as_str()),
        ] {
            require_non_empty(field, value)?;
        }
        Ok(())
    }
}

/// A typed reason for a non-happy-path supervisor lifecycle receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorLifecycleReason {
    CapabilityMissing,
    CheckpointFailed,
    DirectiveExpired,
    OutboxUnavailable,
    ProtocolUnsupported,
    SupervisorFailed,
    WorkerLost,
}

/// A deterministic invalid supervisor-contract declaration.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SupervisorError {
    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },
    #[error("an enrolled supervisor must advertise at least one directive capability")]
    NoDirectiveCapabilities,
    #[error("supervisor advertised directive capability {capability:?} more than once")]
    DuplicateDirectiveCapability {
        capability: SupervisorDirectiveCapability,
    },
    #[error(
        "force termination support ({supports_force_termination}) does not match the advertised force capability ({force_termination_advertised})"
    )]
    ForceTerminationMismatch {
        supports_force_termination: bool,
        force_termination_advertised: bool,
    },
    #[error("an ephemeral outbox cannot claim recovery after process loss")]
    EphemeralOutboxCannotBeRecoverable,
}

fn require_non_empty(field: &'static str, value: &str) -> Result<(), SupervisorError> {
    if value.trim().is_empty() {
        return Err(SupervisorError::EmptyField { field });
    }
    Ok(())
}
