//! The Bridge's typed response vocabulary for ADR-0125 Work commands,
//! translated from [`WorkCommandServiceOutcome`]. Split out of `mod.rs` to
//! keep the routing/handler module focused on the HTTP boundary.

use ackplane_server::work_command_store::{
    WorkCommandOutcome, WorkCommandRefusal, WorkCommandServiceOutcome,
};
use serde::Serialize;

fn refusal_label(reason: WorkCommandRefusal) -> &'static str {
    match reason {
        WorkCommandRefusal::MissingPrincipal => "missing_principal",
        WorkCommandRefusal::ForgedPrincipal => "forged_principal",
        WorkCommandRefusal::TenantOutOfScope => "tenant_out_of_scope",
        WorkCommandRefusal::RepositoryOutOfScope => "repository_out_of_scope",
        WorkCommandRefusal::CommandNotPermitted => "command_not_permitted",
        WorkCommandRefusal::PolicyNotPermitted => "policy_not_permitted",
        WorkCommandRefusal::DelegationNotPermitted => "delegation_not_permitted",
    }
}

fn outcome_label(outcome: WorkCommandOutcome) -> &'static str {
    match outcome {
        WorkCommandOutcome::PendingConfirmation => "pending_confirmation",
        WorkCommandOutcome::PendingDelivery => "pending_delivery",
        WorkCommandOutcome::Accepted => "accepted",
        WorkCommandOutcome::Applied => "applied",
        WorkCommandOutcome::Failed => "failed",
        WorkCommandOutcome::Expired => "expired",
        WorkCommandOutcome::Conflicted => "conflicted",
        WorkCommandOutcome::Refused => "refused",
    }
}

/// The full outcome vocabulary a caller may see (ADR-0125 decision 9): every
/// case is a first-class, named result -- never an HTTP success standing in
/// for "the worker did it" (decision 7).
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum WorkCommandResponse {
    AuthorizationUnavailable {
        reason: &'static str,
    },
    Refused {
        reason: &'static str,
    },
    PendingConfirmation {
        command_id: String,
        receipt_id: String,
        outcome: &'static str,
        idempotent_replay: bool,
    },
    Executed {
        command_id: String,
        receipt_id: String,
        outcome: &'static str,
        reason: String,
        idempotent_replay: bool,
    },
    CommandNotFound,
}

impl From<WorkCommandServiceOutcome> for WorkCommandResponse {
    fn from(outcome: WorkCommandServiceOutcome) -> Self {
        match outcome {
            WorkCommandServiceOutcome::AuthorizationUnavailable { reason } => {
                Self::AuthorizationUnavailable { reason }
            }
            WorkCommandServiceOutcome::Refused { reason } => Self::Refused {
                reason: refusal_label(reason),
            },
            WorkCommandServiceOutcome::PendingConfirmation {
                command,
                receipt,
                idempotent_replay,
            } => Self::PendingConfirmation {
                command_id: command.command_id,
                receipt_id: receipt.receipt_id,
                outcome: outcome_label(receipt.outcome),
                idempotent_replay,
            },
            WorkCommandServiceOutcome::Executed {
                command,
                receipt,
                idempotent_replay,
            } => Self::Executed {
                command_id: command.command_id,
                receipt_id: receipt.receipt_id,
                outcome: outcome_label(receipt.outcome),
                reason: receipt.reason,
                idempotent_replay,
            },
            WorkCommandServiceOutcome::CommandNotFound => Self::CommandNotFound,
        }
    }
}
