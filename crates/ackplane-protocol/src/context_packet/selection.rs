//! What a context packet contains, and the deterministic reason each item is
//! there -- selected, excluded by budget, or rejected before ranking.

use serde::{Deserialize, Serialize};

use super::{require_non_empty, require_optional_non_empty, ContextPacketError};

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
    pub(super) fn validate(&self) -> Result<(), ContextPacketError> {
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
