//! Turning this supervisor's declarations into the wire frames Ackplane reads.
//!
//! Split out of `daemon.rs` so the run loop reads as control flow rather than
//! as struct construction. Every function here is a pure mapping: no I/O, no
//! connection, no clock beyond a caller-supplied instant.

use ackplane_protocol::{
    supervisor::{SupervisorDirectiveCapability, SupervisorRegistration, SupervisorSession},
    v1,
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use super::DaemonError;

pub(super) fn registration_frame(registration: &SupervisorRegistration) -> v1::NodeFrame {
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

pub(super) fn session_frame(
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

pub(super) fn heartbeat_frame(supervisor_id: &str) -> v1::NodeFrame {
    v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::SupervisorHeartbeat(
            v1::SupervisorHeartbeat {
                supervisor_id: supervisor_id.to_string(),
            },
        )),
    }
}
