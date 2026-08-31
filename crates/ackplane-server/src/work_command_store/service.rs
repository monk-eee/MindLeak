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
    payload::WorkCommandPayload,
    WorkCommandStore,
};

pub const AUTHORIZATION_UNAVAILABLE_REASON: &str =
    "The Bridge loopback developer profile has no verified principal or authorization verifier.";

/// A principal result trusted from a future authentication verifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedWorkCommandPrincipal {
    pub principal_id: String,
    pub tenant_id: String,
    pub repository_ids: Vec<String>,
    pub allowed_commands: Vec<WorkCommandKind>,
    pub policy_refs: Vec<String>,
    pub delegation_id: Option<String>,
}

/// The authentication result the service evaluates before touching a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkCommandAuthorization {
    LoopbackDevelopment,
    MissingPrincipal,
    Verified(VerifiedWorkCommandPrincipal),
}

/// A refusal that reveals no Work task, claim, or receipt existence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkCommandRefusal {
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
pub enum WorkCommandServiceOutcome {
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
    /// The confirm step ran (ADR-0125 decision 8); `receipt.outcome` carries
    /// the actual result -- `Applied`, `Conflicted`, or `Refused` -- and
    /// `idempotent_replay` is set when this exact confirmation already ran.
    Executed {
        command: Box<WorkCommand>,
        receipt: Box<WorkCommandReceipt>,
        idempotent_replay: bool,
    },
    /// No command with this id exists in this tenant and repository.
    CommandNotFound,
}

#[derive(Debug, Error)]
pub enum WorkCommandServiceError {
    #[error("work command store error: {0}")]
    Store(#[from] WorkCommandStoreError),
    #[error("work command connection error: {0}")]
    Connection(#[from] tokio_postgres::Error),
}

/// The authoritative, crate-internal command-service boundary.
pub struct WorkCommandService {
    store: WorkCommandStore,
}

impl WorkCommandService {
    pub async fn connect(pool: &crate::db_pool::PgPool) -> Result<Self, WorkCommandServiceError> {
        Ok(Self {
            store: WorkCommandStore::connect(pool).await?,
        })
    }

    /// Checks identity and scope before the immutable command ledger is read or written.
    pub async fn submit(
        &self,
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

    /// Confirms and executes an already-recorded command's server-owned
    /// effect (ADR-0125 decision 8). The confirming principal must be the
    /// same one that issued the command; the payload must be the exact
    /// content the command's digest was fixed against at submission.
    pub async fn confirm(
        &self,
        authorization: WorkCommandAuthorization,
        tenant_id: &str,
        repository_id: &str,
        command_id: &str,
        payload: WorkCommandPayload,
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
        let Some(command) = self
            .store
            .find_command(tenant_id, repository_id, command_id)
            .await?
        else {
            return Ok(WorkCommandServiceOutcome::CommandNotFound);
        };
        if principal.principal_id != command.issuing_principal_id
            || principal.tenant_id != command.tenant_id
            || !principal.repository_ids.contains(&command.repository_id)
        {
            return Ok(WorkCommandServiceOutcome::Refused {
                reason: WorkCommandRefusal::ForgedPrincipal,
            });
        }
        let outcome = self
            .store
            .execute_confirmed(&command, &payload, now)
            .await?;
        Ok(WorkCommandServiceOutcome::Executed {
            command: Box::new(command),
            receipt: Box::new(outcome.receipt),
            idempotent_replay: outcome.idempotent_replay,
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
    // Bug fix: this used to also refuse whenever `principal.policy_refs` was
    // empty, regardless of what the request asked for. That made a
    // no-adopted-policy principal (ADR-0142 clause 5: a self-hosted Work
    // command principal deliberately carries no `AdministrationPolicy`-style
    // policy layer) permanently unable to issue any command, even one that
    // itself named no policy either -- the exact self-hosted profile ADR-0142
    // exists to unlock. A plain equality check keeps refusing a request that
    // claims a policy basis the principal does not have (or a different one),
    // while correctly allowing "neither side claims a policy" through.
    if principal.policy_refs != request.policy_refs {
        return Some(WorkCommandRefusal::PolicyNotPermitted);
    }
    if principal.delegation_id != request.delegation_id {
        return Some(WorkCommandRefusal::DelegationNotPermitted);
    }
    None
}
