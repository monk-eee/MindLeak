//! Storage-independent, human-issued delegation contracts for Industrial agents.
//!
//! ADR-0115 makes a human-approved delegation the bounded authority envelope
//! for routine work. This module records that envelope and its receipt only; a
//! future durable authorization service owns principal verification, policy
//! evaluation, revocation, and command enforcement.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The tenant and repository scope within which one delegation may be used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationScope {
    pub tenant_id: String,
    pub repository_id: String,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
}

impl DelegationScope {
    pub fn validate(&self) -> Result<(), DelegationError> {
        require_non_empty("tenant_id", &self.tenant_id)?;
        require_non_empty("repository_id", &self.repository_id)?;
        validate_optional_scope("project_id", self.project_id.as_deref())?;
        validate_optional_scope("task_id", self.task_id.as_deref())
    }
}

/// A bounded, human-issued grant for one enrolled agent session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanDelegation {
    pub delegation_id: String,
    pub issuer_principal_id: String,
    pub delegatee_session_id: String,
    pub scope: DelegationScope,
    pub policy_version: String,
    pub allowed_actions: Vec<DelegatedAction>,
    pub max_token_budget: u32,
    pub max_actions_per_session: u32,
    pub issued_at: i64,
    pub effective_at: i64,
    pub expires_at: i64,
    pub status: DelegationStatus,
}

impl HumanDelegation {
    /// Reject declarations that would make a delegation broader or less truthful than stated.
    pub fn validate(&self) -> Result<(), DelegationError> {
        for (field, value) in [
            ("delegation_id", self.delegation_id.as_str()),
            ("issuer_principal_id", self.issuer_principal_id.as_str()),
            ("delegatee_session_id", self.delegatee_session_id.as_str()),
            ("policy_version", self.policy_version.as_str()),
        ] {
            require_non_empty(field, value)?;
        }
        self.scope.validate()?;

        if self.allowed_actions.is_empty() {
            return Err(DelegationError::NoDelegatedActions);
        }
        let mut actions = HashSet::new();
        for action in &self.allowed_actions {
            if !action.is_routine() {
                return Err(DelegationError::NonRoutineAction { action: *action });
            }
            if !actions.insert(*action) {
                return Err(DelegationError::DuplicateDelegatedAction { action: *action });
            }
        }

        if self.max_token_budget == 0 {
            return Err(DelegationError::NonPositiveLimit {
                field: "max_token_budget",
            });
        }
        if self.max_actions_per_session == 0 {
            return Err(DelegationError::NonPositiveLimit {
                field: "max_actions_per_session",
            });
        }
        if self.effective_at < self.issued_at {
            return Err(DelegationError::EffectiveBeforeIssued);
        }
        if self.expires_at <= self.effective_at {
            return Err(DelegationError::ExpiryMustFollowEffectivity);
        }

        Ok(())
    }
}

/// Routine actions a human delegation may authorize in this first contract.
///
/// The closed enum intentionally has no policy, Constitution, waiver, identity,
/// sensitive-export, force-termination, or arbitrary-execution action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegatedAction {
    RetrieveContext,
    Analyze,
    ClaimTask,
    WorkTask,
    CreateCandidateKnowledge,
    RunValidation,
    ReportEvidence,
}

impl DelegatedAction {
    fn is_routine(self) -> bool {
        matches!(
            self,
            Self::RetrieveContext
                | Self::Analyze
                | Self::ClaimTask
                | Self::WorkTask
                | Self::CreateCandidateKnowledge
                | Self::RunValidation
                | Self::ReportEvidence
        )
    }
}

/// The durable state a future authorization service may project for a delegation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationStatus {
    Active,
    Revoked,
    Expired,
}

/// An attributed observation of a delegation decision or terminal lifecycle change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationReceipt {
    pub delegation_id: String,
    pub issuer_principal_id: String,
    pub delegatee_session_id: String,
    pub scope: DelegationScope,
    pub occurred_at: i64,
    pub status: DelegationReceiptStatus,
    pub reason: Option<DelegationReceiptReason>,
}

impl DelegationReceipt {
    pub fn validate(&self) -> Result<(), DelegationError> {
        for (field, value) in [
            ("delegation_id", self.delegation_id.as_str()),
            ("issuer_principal_id", self.issuer_principal_id.as_str()),
            ("delegatee_session_id", self.delegatee_session_id.as_str()),
        ] {
            require_non_empty(field, value)?;
        }
        self.scope.validate()
    }
}

/// The terminal or currently-effective state reported in a delegation receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationReceiptStatus {
    Granted,
    Refused,
    Revoked,
    Expired,
}

/// A typed reason for a refused or terminal delegation receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationReceiptReason {
    PolicyDenied,
    ScopeDenied,
    LimitDenied,
    Expired,
    Revoked,
}

/// A deterministic invalid delegation-contract declaration.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DelegationError {
    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },
    #[error("optional scope field {field} must be absent or non-empty")]
    BlankOptionalScope { field: &'static str },
    #[error("a delegation must allow at least one routine action")]
    NoDelegatedActions,
    #[error("delegated action {action:?} appears more than once")]
    DuplicateDelegatedAction { action: DelegatedAction },
    #[error("delegated action {action:?} is outside the routine-only vocabulary")]
    NonRoutineAction { action: DelegatedAction },
    #[error("{field} must be greater than zero")]
    NonPositiveLimit { field: &'static str },
    #[error("delegation effective time cannot precede issuance")]
    EffectiveBeforeIssued,
    #[error("delegation expiry must follow its effective time")]
    ExpiryMustFollowEffectivity,
}

fn require_non_empty(field: &'static str, value: &str) -> Result<(), DelegationError> {
    if value.trim().is_empty() {
        return Err(DelegationError::EmptyField { field });
    }
    Ok(())
}

fn validate_optional_scope(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), DelegationError> {
    if value.is_some_and(|value| value.trim().is_empty()) {
        return Err(DelegationError::BlankOptionalScope { field });
    }
    Ok(())
}
