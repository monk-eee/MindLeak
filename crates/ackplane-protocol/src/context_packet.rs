//! Versioned, storage-independent ContextPacket types for the Industrial guidance loop.
//!
//! A packet is an immutable, bounded, attributed audit record. It is context,
//! not an authorization: task commands must still check their live authority.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// The protocol revision for packets with complete audit attribution.
pub const CONTEXT_PACKET_PROTOCOL_VERSION: &str = "ackplane.context_packet/v2";

/// An immutable, bounded snapshot of context compiled for one agent task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPacket {
    pub packet_id: String,
    /// SHA-256 of this packet's canonical serialized content, excluding this field.
    pub digest: String,
    pub protocol_version: String,
    pub scope: ContextPacketScope,
    /// An Industrial project when the repository is organized under one.
    pub project_id: Option<String>,
    pub compiler_version: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub source: ContextPacketSource,
    pub token_budget: ContextTokenBudget,
    /// The immutable packet state at compilation. Delivery/use progress belongs
    /// to separate [`ContextPacketUseReceipt`] records.
    pub lifecycle: ContextPacketLifecycle,
    pub selected: Vec<ContextSelection>,
    pub budget_excluded: Vec<ContextBudgetExclusion>,
    pub rejected: Vec<ContextCandidateRejection>,
}

impl ContextPacket {
    /// Seals content with its deterministic digest after validating every
    /// structural invariant. Callers must seal a newly assembled packet before
    /// it can be stored or delivered.
    pub fn seal(mut self) -> Result<Self, ContextPacketError> {
        self.validate_content()?;
        self.digest = self.computed_digest()?;
        self.validate()?;
        Ok(self)
    }

    /// Returns the digest that the current content should carry.
    pub fn computed_digest(&self) -> Result<String, ContextPacketError> {
        let content = ContextPacketDigestInput {
            packet_id: &self.packet_id,
            protocol_version: &self.protocol_version,
            scope: &self.scope,
            project_id: &self.project_id,
            compiler_version: &self.compiler_version,
            issued_at: self.issued_at,
            expires_at: self.expires_at,
            source: &self.source,
            token_budget: &self.token_budget,
            lifecycle: self.lifecycle,
            selected: &self.selected,
            budget_excluded: &self.budget_excluded,
            rejected: &self.rejected,
        };
        let encoded = serde_json::to_vec(&content).map_err(|error| {
            ContextPacketError::DigestSerialization {
                detail: error.to_string(),
            }
        })?;
        let digest = Sha256::digest(encoded);
        Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    /// Reject malformed or tampered packet data before it reaches transport or storage.
    pub fn validate(&self) -> Result<(), ContextPacketError> {
        self.validate_content()?;
        require_non_empty("digest", &self.digest)?;
        let expected = self.computed_digest()?;
        if self.digest != expected {
            return Err(ContextPacketError::DigestMismatch {
                expected,
                actual: self.digest.clone(),
            });
        }
        Ok(())
    }

    fn validate_content(&self) -> Result<(), ContextPacketError> {
        require_non_empty("packet_id", &self.packet_id)?;
        require_non_empty("protocol_version", &self.protocol_version)?;
        require_optional_non_empty("project_id", self.project_id.as_deref())?;
        require_non_empty("compiler_version", &self.compiler_version)?;
        self.scope.validate()?;

        if self.expires_at <= self.issued_at {
            return Err(ContextPacketError::ExpiryMustFollowIssuance);
        }
        if self.token_budget.used > self.token_budget.requested {
            return Err(ContextPacketError::TokenBudgetExceeded {
                requested: self.token_budget.requested,
                used: self.token_budget.used,
            });
        }

        let mut item_ids = HashSet::new();
        let mut satisfied_requirements = HashSet::new();
        for selection in &self.selected {
            selection.validate()?;
            if !item_ids.insert(selection.item_id.as_str()) {
                return Err(ContextPacketError::DuplicateSelectedItem {
                    item_id: selection.item_id.clone(),
                });
            }
            if selection.mandatory {
                let Some(requirement) = selection.reason.mandatory_requirement() else {
                    return Err(ContextPacketError::MandatorySelectionMissingRequirement {
                        item_id: selection.item_id.clone(),
                    });
                };
                if !requirement.accepts(selection.item_kind) {
                    return Err(ContextPacketError::MandatorySelectionKindMismatch {
                        item_id: selection.item_id.clone(),
                        item_kind: selection.item_kind,
                        requirement,
                    });
                }
                satisfied_requirements.insert(requirement);
            }
        }

        for requirement in ContextMandatoryRequirement::ALL {
            if !satisfied_requirements.contains(&requirement) {
                return Err(ContextPacketError::MissingMandatoryEnvelopeRequirement {
                    requirement,
                });
            }
        }

        for exclusion in &self.budget_excluded {
            exclusion.validate()?;
            if !item_ids.insert(exclusion.item_id.as_str()) {
                return Err(ContextPacketError::DuplicatePacketItem {
                    item_id: exclusion.item_id.clone(),
                });
            }
        }

        for rejection in &self.rejected {
            rejection.validate()?;
            if !item_ids.insert(rejection.item_id.as_str()) {
                return Err(ContextPacketError::DuplicatePacketItem {
                    item_id: rejection.item_id.clone(),
                });
            }
        }

        let selected_tokens = self.selected.iter().try_fold(0_u32, |total, selection| {
            total
                .checked_add(selection.estimated_tokens)
                .ok_or(ContextPacketError::TokenBudgetArithmeticOverflow)
        })?;
        if selected_tokens != self.token_budget.used {
            return Err(ContextPacketError::TokenBudgetUsageMismatch {
                declared: self.token_budget.used,
                selected: selected_tokens,
            });
        }

        Ok(())
    }
}

/// The tenant, repository, task, goal, and agent session one packet may serve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPacketScope {
    pub tenant_id: String,
    pub repository_id: String,
    pub task_id: String,
    pub goal_id: String,
    pub agent_session_id: String,
}

impl ContextPacketScope {
    pub fn validate(&self) -> Result<(), ContextPacketError> {
        for (field, value) in [
            ("tenant_id", self.tenant_id.as_str()),
            ("repository_id", self.repository_id.as_str()),
            ("task_id", self.task_id.as_str()),
            ("goal_id", self.goal_id.as_str()),
            ("agent_session_id", self.agent_session_id.as_str()),
        ] {
            require_non_empty(field, value)?;
        }
        Ok(())
    }
}

/// The authoritative positions from which a packet was compiled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPacketSource {
    pub ledger_position: u64,
    pub projection_position: u64,
}

/// Requested and actual token use for one packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextTokenBudget {
    pub requested: u32,
    pub used: u32,
}

/// The immutable lifecycle state recorded by a newly compiled packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPacketLifecycle {
    Compiled,
}

/// The tenant/repository and optional work scope from which one item came.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextItemScope {
    pub tenant_id: String,
    pub repository_id: String,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub goal_id: Option<String>,
}

impl ContextItemScope {
    fn validate(&self) -> Result<(), ContextPacketError> {
        require_non_empty("selected.source_scope.tenant_id", &self.tenant_id)?;
        require_non_empty("selected.source_scope.repository_id", &self.repository_id)?;
        require_optional_non_empty(
            "selected.source_scope.project_id",
            self.project_id.as_deref(),
        )?;
        require_optional_non_empty("selected.source_scope.task_id", self.task_id.as_deref())?;
        require_optional_non_empty("selected.source_scope.goal_id", self.goal_id.as_deref())
    }
}

/// The attributed producer and evidence chain for one rendered item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextProvenance {
    pub recorded_by: String,
    pub recorded_at: i64,
    pub evidence_reference: Option<String>,
}

impl ContextProvenance {
    fn validate(&self) -> Result<(), ContextPacketError> {
        require_non_empty("selected.provenance.recorded_by", &self.recorded_by)?;
        require_optional_non_empty(
            "selected.provenance.evidence_reference",
            self.evidence_reference.as_deref(),
        )
    }
}

/// The interval during which a rendered source item remains current.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextFreshness {
    pub observed_at: i64,
    pub expires_at: Option<i64>,
}

impl ContextFreshness {
    fn validate(&self) -> Result<(), ContextPacketError> {
        if self
            .expires_at
            .is_some_and(|expires_at| expires_at <= self.observed_at)
        {
            return Err(ContextPacketError::FreshnessExpiryMustFollowObservation);
        }
        Ok(())
    }
}

/// One source item included in a packet and why it was selected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSelection {
    pub item_id: String,
    pub item_kind: ContextItemKind,
    pub source_reference: String,
    pub source_scope: ContextItemScope,
    pub provenance: ContextProvenance,
    pub freshness: ContextFreshness,
    pub source_version: String,
    /// The bounded, recipient-ready rendering of the source item.
    pub rendered: String,
    pub reason: ContextSelectionReason,
    /// Present only when optional ranking determined this item's inclusion.
    pub effective_relevance: Option<u64>,
    pub estimated_tokens: u32,
    pub mandatory: bool,
}

impl ContextSelection {
    /// Validates the item-level audit data carried by a packet selection.
    pub fn validate(&self) -> Result<(), ContextPacketError> {
        require_non_empty("selected.item_id", &self.item_id)?;
        require_non_empty("selected.source_reference", &self.source_reference)?;
        require_non_empty("selected.source_version", &self.source_version)?;
        require_non_empty("selected.rendered", &self.rendered)?;
        self.source_scope.validate()?;
        self.provenance.validate()?;
        self.freshness.validate()?;
        if self.estimated_tokens == 0 {
            return Err(ContextPacketError::ZeroTokenEstimate {
                item_id: self.item_id.clone(),
            });
        }

        match (self.mandatory, self.reason.mandatory_requirement()) {
            (true, Some(_)) if self.effective_relevance.is_none() => Ok(()),
            (true, Some(_)) => Err(ContextPacketError::MandatorySelectionHasRelevance {
                item_id: self.item_id.clone(),
            }),
            (true, None) => Err(ContextPacketError::MandatorySelectionMissingRequirement {
                item_id: self.item_id.clone(),
            }),
            (false, Some(_)) => Err(ContextPacketError::OptionalSelectionUsesMandatoryReason {
                item_id: self.item_id.clone(),
            }),
            (false, None) if self.effective_relevance.is_some() => Ok(()),
            (false, None) => Err(ContextPacketError::OptionalSelectionMissingRelevance {
                item_id: self.item_id.clone(),
            }),
        }
    }
}

/// The typed source a context packet may include.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextItemKind {
    TargetIdentity,
    Objective,
    Acceptance,
    Constitution,
    Policy,
    SafetyControl,
    EvidenceCondition,
    TaskLease,
    Evidence,
    Knowledge,
    Outcome,
    Structural,
}

/// Mandatory material that must fit before optional candidates are ranked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextMandatoryRequirement {
    TargetIdentity,
    TaskLease,
    Objective,
    Acceptance,
    Constitution,
    Policy,
    SafetyControl,
    EvidenceCondition,
}

impl ContextMandatoryRequirement {
    /// Whether this required envelope slot accepts the supplied typed item.
    pub fn accepts(self, item_kind: ContextItemKind) -> bool {
        matches!(
            (self, item_kind),
            (Self::TargetIdentity, ContextItemKind::TargetIdentity)
                | (Self::TaskLease, ContextItemKind::TaskLease)
                | (Self::Objective, ContextItemKind::Objective)
                | (Self::Acceptance, ContextItemKind::Acceptance)
                | (Self::Constitution, ContextItemKind::Constitution)
                | (Self::Policy, ContextItemKind::Policy)
                | (Self::SafetyControl, ContextItemKind::SafetyControl)
                | (Self::EvidenceCondition, ContextItemKind::EvidenceCondition)
        )
    }

    /// Every packet compiled by Ackplane reserves these audit and safety slots.
    pub const ALL: [Self; 8] = [
        Self::TargetIdentity,
        Self::TaskLease,
        Self::Objective,
        Self::Acceptance,
        Self::Constitution,
        Self::Policy,
        Self::SafetyControl,
        Self::EvidenceCondition,
    ];
}

/// The deterministic reason a source item was selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSelectionReason {
    RequiredTargetIdentity,
    RequiredTaskLease,
    RequiredObjective,
    RequiredAcceptance,
    RequiredConstitution,
    RequiredPolicy,
    RequiredSafetyControl,
    RequiredEvidenceCondition,
    ExplicitTaskReference,
    EvidenceLink,
    GraphReach,
    SemanticSimilarity,
    WorkingSet,
    PriorOutcome,
}

impl ContextSelectionReason {
    /// The required envelope slot this reason represents, when it is mandatory.
    pub fn mandatory_requirement(self) -> Option<ContextMandatoryRequirement> {
        match self {
            Self::RequiredTargetIdentity => Some(ContextMandatoryRequirement::TargetIdentity),
            Self::RequiredTaskLease => Some(ContextMandatoryRequirement::TaskLease),
            Self::RequiredObjective => Some(ContextMandatoryRequirement::Objective),
            Self::RequiredAcceptance => Some(ContextMandatoryRequirement::Acceptance),
            Self::RequiredConstitution => Some(ContextMandatoryRequirement::Constitution),
            Self::RequiredPolicy => Some(ContextMandatoryRequirement::Policy),
            Self::RequiredSafetyControl => Some(ContextMandatoryRequirement::SafetyControl),
            Self::RequiredEvidenceCondition => Some(ContextMandatoryRequirement::EvidenceCondition),
            Self::ExplicitTaskReference
            | Self::EvidenceLink
            | Self::GraphReach
            | Self::SemanticSimilarity
            | Self::WorkingSet
            | Self::PriorOutcome => None,
        }
    }
}

/// The deterministic inputs used to rank an optional candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextRankingInputs {
    pub effective_relevance: u64,
    pub estimated_tokens: u32,
    pub stable_tie_breaker: String,
}

impl ContextRankingInputs {
    fn validate(&self, item_id: &str) -> Result<(), ContextPacketError> {
        if self.estimated_tokens == 0 {
            return Err(ContextPacketError::ZeroTokenEstimate {
                item_id: item_id.to_string(),
            });
        }
        require_non_empty(
            "budget_excluded.ranking.stable_tie_breaker",
            &self.stable_tie_breaker,
        )?;
        if self.stable_tie_breaker != item_id {
            return Err(ContextPacketError::RankingTieBreakerMismatch {
                item_id: item_id.to_string(),
                tie_breaker: self.stable_tie_breaker.clone(),
            });
        }
        Ok(())
    }
}

/// An optional candidate deliberately omitted because its complete item did not
/// fit the remaining token budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextBudgetExclusion {
    pub item_id: String,
    pub item_kind: ContextItemKind,
    pub ranking: ContextRankingInputs,
    pub reason: ContextBudgetExclusionReason,
}

impl ContextBudgetExclusion {
    fn validate(&self) -> Result<(), ContextPacketError> {
        require_non_empty("budget_excluded.item_id", &self.item_id)?;
        self.ranking.validate(&self.item_id)
    }
}

/// The reason an otherwise eligible optional candidate exceeded the budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextBudgetExclusionReason {
    Budget,
}

/// A candidate rejected before budget ranking because it could not safely
/// become context for this target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextCandidateRejection {
    pub item_id: String,
    pub item_kind: ContextItemKind,
    pub source_reference: String,
    pub source_version: String,
    pub reason: ContextCandidateRejectionReason,
}

impl ContextCandidateRejection {
    /// Validates the minimal audit data retained for a pre-budget rejection.
    pub fn validate(&self) -> Result<(), ContextPacketError> {
        require_non_empty("rejected.item_id", &self.item_id)?;
        require_non_empty("rejected.source_reference", &self.source_reference)?;
        require_non_empty("rejected.source_version", &self.source_version)
    }
}

/// Why a candidate was rejected before token-budget selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCandidateRejectionReason {
    Unauthorized,
    StaleBeyondPolicy,
    Retired,
    OutOfScope,
    MissingRequiredEvidence,
}

/// A supervisor's attributed observation of how it handled one packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPacketUseReceipt {
    pub packet_id: String,
    pub scope: ContextPacketScope,
    pub occurred_at: i64,
    pub status: ContextPacketUseStatus,
    pub reason: Option<ContextPacketUseReason>,
}

impl ContextPacketUseReceipt {
    pub fn validate(&self) -> Result<(), ContextPacketError> {
        require_non_empty("packet_id", &self.packet_id)?;
        self.scope.validate()
    }
}

/// The observed lifecycle state of a delivered context packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPacketUseStatus {
    Received,
    Accepted,
    AppliedToPlanning,
    Superseded,
    Refused,
    Expired,
}

/// The typed reason attached to a non-happy-path packet-use receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPacketUseReason {
    Superseded,
    UnsupportedVersion,
    OutOfScope,
    IntegrityMismatch,
    PolicyChanged,
}

/// A deterministic packet-contract validation refusal.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContextPacketError {
    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },
    #[error("context packet expiry must follow issuance")]
    ExpiryMustFollowIssuance,
    #[error("context item freshness expiry must follow its observation")]
    FreshnessExpiryMustFollowObservation,
    #[error("context packet used {used} tokens but its budget is {requested}")]
    TokenBudgetExceeded { requested: u32, used: u32 },
    #[error(
        "context packet declares {declared} used tokens but its selected items require {selected}"
    )]
    TokenBudgetUsageMismatch { declared: u32, selected: u32 },
    #[error("context packet token arithmetic overflowed")]
    TokenBudgetArithmeticOverflow,
    #[error("context packet selected item {item_id:?} more than once")]
    DuplicateSelectedItem { item_id: String },
    #[error("context packet item {item_id:?} appears in more than one disposition")]
    DuplicatePacketItem { item_id: String },
    #[error("context item {item_id:?} must have a non-zero token estimate")]
    ZeroTokenEstimate { item_id: String },
    #[error("mandatory context item {item_id:?} does not name a required envelope slot")]
    MandatorySelectionMissingRequirement { item_id: String },
    #[error(
        "mandatory context item {item_id:?} is {item_kind:?}, which cannot satisfy {requirement:?}"
    )]
    MandatorySelectionKindMismatch {
        item_id: String,
        item_kind: ContextItemKind,
        requirement: ContextMandatoryRequirement,
    },
    #[error("context packet omits required {requirement:?} material")]
    MissingMandatoryEnvelopeRequirement {
        requirement: ContextMandatoryRequirement,
    },
    #[error("mandatory context item {item_id:?} must not carry optional relevance")]
    MandatorySelectionHasRelevance { item_id: String },
    #[error("optional context item {item_id:?} cannot use a mandatory selection reason")]
    OptionalSelectionUsesMandatoryReason { item_id: String },
    #[error("optional context item {item_id:?} must record its effective relevance")]
    OptionalSelectionMissingRelevance { item_id: String },
    #[error(
        "budget-excluded item {item_id:?} must use its own identifier as the stable tie-breaker, not {tie_breaker:?}"
    )]
    RankingTieBreakerMismatch {
        item_id: String,
        tie_breaker: String,
    },
    #[error("context packet digest serialization failed: {detail}")]
    DigestSerialization { detail: String },
    #[error("context packet digest does not match its content")]
    DigestMismatch { expected: String, actual: String },
}

#[derive(Serialize)]
struct ContextPacketDigestInput<'a> {
    packet_id: &'a str,
    protocol_version: &'a str,
    scope: &'a ContextPacketScope,
    project_id: &'a Option<String>,
    compiler_version: &'a str,
    issued_at: i64,
    expires_at: i64,
    source: &'a ContextPacketSource,
    token_budget: &'a ContextTokenBudget,
    lifecycle: ContextPacketLifecycle,
    selected: &'a [ContextSelection],
    budget_excluded: &'a [ContextBudgetExclusion],
    rejected: &'a [ContextCandidateRejection],
}

fn require_non_empty(field: &'static str, value: &str) -> Result<(), ContextPacketError> {
    if value.trim().is_empty() {
        return Err(ContextPacketError::EmptyField { field });
    }
    Ok(())
}

fn require_optional_non_empty(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), ContextPacketError> {
    if let Some(value) = value {
        require_non_empty(field, value)?;
    }
    Ok(())
}
