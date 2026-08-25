//! Authorization boundary for Industrial Work commands (ADR-0125).
//!
//! This service owns access to [`WorkCommandStore`]. It has no transport and
//! does not mutate Work or Claim state; those effects remain later slices.

use std::time::SystemTime;

use thiserror::Error;

use super::{
    model::{
        NewWorkCommand, NewWorkCommandReceipt, WorkCommand, WorkCommandKind, WorkCommandOutcome,
        WorkCommandReceipt, WorkCommandStoreError,
    },
    WorkCommandStore,
};

pub(super) const AUTHORIZATION_UNAVAILABLE_REASON: &str =
    "The Bridge loopback developer profile has no verified principal or authorization verifier.";

/// A principal result trusted from a future authentication verifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VerifiedWorkCommandPrincipal {
    pub principal_id: String,
    pub tenant_id: String,
    pub repository_ids: Vec<String>,
    pub allowed_commands: Vec<WorkCommandKind>,
    pub policy_refs: Vec<String>,
    pub delegation_id: Option<String>,
}

/// The authentication result the service evaluates before touching a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WorkCommandAuthorization {
    LoopbackDevelopment,
    MissingPrincipal,
    Verified(VerifiedWorkCommandPrincipal),
}

/// A refusal that reveals no Work task, claim, or receipt existence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkCommandRefusal {
    MissingPrincipal,
    ForgedPrincipal,
    TenantOutOfScope,
    RepositoryOutOfScope,
    CommandNotPermitted,
    PolicyNotPermitted,
    DelegationNotPermitted,
}

/// The command-service outcome before any Work or Claim state transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WorkCommandServiceOutcome {
    AuthorizationUnavailable {
        reason: &'static str,
    },
    Refused {
        reason: WorkCommandRefusal,
    },
    PendingConfirmation {
        command: Box<WorkCommand>,
        receipt: Box<WorkCommandReceipt>,
        idempotent_replay: bool,
    },
}

#[derive(Debug, Error)]
pub(super) enum WorkCommandServiceError {
    #[error("work command store error: {0}")]
    Store(#[from] WorkCommandStoreError),
    #[error("work command connection error: {0}")]
    Connection(#[from] tokio_postgres::Error),
}

/// The authoritative, crate-internal command-service boundary.
pub(super) struct WorkCommandService {
    store: WorkCommandStore,
}

impl WorkCommandService {
    pub(super) async fn connect(database_url: &str) -> Result<Self, WorkCommandServiceError> {
        Ok(Self {
            store: WorkCommandStore::connect(database_url).await?,
        })
    }

    /// Checks identity and scope before the immutable command ledger is read or written.
    pub(super) async fn submit(
        &mut self,
        authorization: WorkCommandAuthorization,
        request: NewWorkCommand,
        now: SystemTime,
    ) -> Result<WorkCommandServiceOutcome, WorkCommandServiceError> {
        let principal = match authorization {
            WorkCommandAuthorization::LoopbackDevelopment => {
                return Ok(WorkCommandServiceOutcome::AuthorizationUnavailable {
                    reason: AUTHORIZATION_UNAVAILABLE_REASON,
                });
            }
            WorkCommandAuthorization::MissingPrincipal => {
                return Ok(WorkCommandServiceOutcome::Refused {
                    reason: WorkCommandRefusal::MissingPrincipal,
                });
            }
            WorkCommandAuthorization::Verified(principal) => principal,
        };

        if let Some(reason) = refusal_for(&principal, &request) {
            return Ok(WorkCommandServiceOutcome::Refused { reason });
        }

        let command = self.store.record_request(&request, now).await?;
        let receipt = self
            .store
            .record_receipt(
                &NewWorkCommandReceipt {
                    tenant_id: request.tenant_id,
                    repository_id: request.repository_id,
                    command_id: command.command.command_id.clone(),
                    receipt_id: format!("{}:pending-confirmation", command.command.command_id),
                    outcome: WorkCommandOutcome::PendingConfirmation,
                    reason: "A verified principal and policy basis passed initial scope checks; confirmation is required before effect.".to_string(),
                    evidence_refs: request.policy_refs,
                    occurred_at: now,
                },
                now,
            )
            .await?;

        Ok(WorkCommandServiceOutcome::PendingConfirmation {
            command: Box::new(command.command),
            receipt: Box::new(receipt.receipt),
            idempotent_replay: command.idempotent_replay && receipt.idempotent_replay,
        })
    }
}

fn refusal_for(
    principal: &VerifiedWorkCommandPrincipal,
    request: &NewWorkCommand,
) -> Option<WorkCommandRefusal> {
    if principal.principal_id != request.issuing_principal_id {
        return Some(WorkCommandRefusal::ForgedPrincipal);
    }
    if principal.tenant_id != request.tenant_id {
        return Some(WorkCommandRefusal::TenantOutOfScope);
    }
    if !principal.repository_ids.contains(&request.repository_id) {
        return Some(WorkCommandRefusal::RepositoryOutOfScope);
    }
    if !principal.allowed_commands.contains(&request.kind) {
        return Some(WorkCommandRefusal::CommandNotPermitted);
    }
    if principal.policy_refs.is_empty() || principal.policy_refs != request.policy_refs {
        return Some(WorkCommandRefusal::PolicyNotPermitted);
    }
    if principal.delegation_id != request.delegation_id {
        return Some(WorkCommandRefusal::DelegationNotPermitted);
    }
    None
}
