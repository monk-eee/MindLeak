//! Deterministic ContextPacket compilation (ADR-0114 decisions 1-4).
//!
//! This module compiles typed candidates into the protocol's immutable packet
//! shape. It neither stores packets nor authenticates a requester; later
//! service slices own those boundaries.

use std::collections::HashSet;

use ackplane_protocol::context_packet::{
    ContextBudgetExclusion, ContextBudgetExclusionReason, ContextCandidateRejection,
    ContextItemKind, ContextItemScope, ContextMandatoryRequirement, ContextPacket,
    ContextPacketError, ContextPacketLifecycle, ContextPacketScope, ContextPacketSource,
    ContextProvenance, ContextRankingInputs, ContextSelection, ContextSelectionReason,
    ContextTokenBudget,
};
use thiserror::Error;

/// One typed candidate the deterministic compiler may select into a packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPacketCandidate {
    pub item_id: String,
    pub item_kind: ContextItemKind,
    pub source_reference: String,
    pub source_scope: ContextItemScope,
    pub provenance: ContextProvenance,
    pub freshness: ackplane_protocol::context_packet::ContextFreshness,
    pub source_version: String,
    pub rendered: String,
    pub reason: ContextSelectionReason,
    pub estimated_tokens: u32,
    pub relevance: u64,
}

/// Inputs required to compile one immutable packet without a model call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPacketCompilationRequest {
    pub packet_id: String,
    pub protocol_version: String,
    pub scope: ContextPacketScope,
    pub project_id: Option<String>,
    pub compiler_version: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub source: ContextPacketSource,
    pub token_budget: u32,
    /// Material the safety envelope requires before optional ranking begins.
    pub mandatory: Vec<ContextPacketCandidate>,
    /// Eligible candidates ranked deterministically by relevance and identifier.
    pub optional: Vec<ContextPacketCandidate>,
    /// Candidates screened out before budget ranking for a typed policy reason.
    pub rejected: Vec<ContextCandidateRejection>,
}

#[derive(Debug, Error)]
pub enum ContextPacketCompilerError {
    #[error("context packet candidate {item_id:?} must have a non-zero token estimate")]
    ZeroTokenEstimate { item_id: String },
    #[error(
        "context packet candidate {item_id:?} expired at {expires_at} before compilation at {issued_at}"
    )]
    StaleCandidate {
        item_id: String,
        issued_at: i64,
        expires_at: i64,
    },
    #[error("context packet candidate {item_id:?} appears more than once")]
    DuplicateCandidate { item_id: String },
    #[error("mandatory candidate {item_id:?} does not name a required envelope slot")]
    MandatoryCandidateMissingRequirement { item_id: String },
    #[error(
        "mandatory candidate {item_id:?} is {item_kind:?}, which cannot satisfy {requirement:?}"
    )]
    InvalidMandatoryKind {
        item_id: String,
        item_kind: ContextItemKind,
        requirement: ContextMandatoryRequirement,
    },
    #[error("context packet omits required {requirement:?} material")]
    MissingMandatoryRequirement {
        requirement: ContextMandatoryRequirement,
    },
    #[error("mandatory candidates require {required} tokens but the packet budget is {requested}")]
    MandatoryBudgetExceeded { requested: u32, required: u32 },
    #[error("context packet token arithmetic overflowed")]
    TokenBudgetOverflow,
    #[error("compiled context packet is invalid: {0}")]
    Packet(#[from] ContextPacketError),
}

/// Compiles a packet by reserving the complete mandatory envelope before
/// ranking optional context. It never substitutes a relevant optional item for
/// a required safety, policy, acceptance, or identity item.
pub fn compile_context_packet(
    mut request: ContextPacketCompilationRequest,
) -> Result<ContextPacket, ContextPacketCompilerError> {
    request
        .mandatory
        .sort_by(|left, right| left.item_id.cmp(&right.item_id));
    request.optional.sort_by(|left, right| {
        right
            .relevance
            .cmp(&left.relevance)
            .then_with(|| left.item_id.cmp(&right.item_id))
    });
    request
        .rejected
        .sort_by(|left, right| left.item_id.cmp(&right.item_id));

    validate_candidates(
        &request.mandatory,
        &request.optional,
        &request.rejected,
        request.issued_at,
    )?;

    let mandatory_tokens = request
        .mandatory
        .iter()
        .try_fold(0_u32, |total, candidate| {
            total
                .checked_add(candidate.estimated_tokens)
                .ok_or(ContextPacketCompilerError::TokenBudgetOverflow)
        })?;
    if mandatory_tokens > request.token_budget {
        return Err(ContextPacketCompilerError::MandatoryBudgetExceeded {
            requested: request.token_budget,
            required: mandatory_tokens,
        });
    }

    let mut used_tokens = mandatory_tokens;
    let mut selected: Vec<ContextSelection> = request
        .mandatory
        .into_iter()
        .map(|candidate| selection_from(candidate, true))
        .collect();
    let mut budget_excluded = Vec::new();

    for candidate in request.optional {
        let Some(next_total) = used_tokens.checked_add(candidate.estimated_tokens) else {
            return Err(ContextPacketCompilerError::TokenBudgetOverflow);
        };
        if next_total <= request.token_budget {
            used_tokens = next_total;
            selected.push(selection_from(candidate, false));
        } else {
            budget_excluded.push(ContextBudgetExclusion {
                item_id: candidate.item_id.clone(),
                item_kind: candidate.item_kind,
                ranking: ContextRankingInputs {
                    effective_relevance: candidate.relevance,
                    estimated_tokens: candidate.estimated_tokens,
                    stable_tie_breaker: candidate.item_id,
                },
                reason: ContextBudgetExclusionReason::Budget,
            });
        }
    }

    ContextPacket {
        packet_id: request.packet_id,
        digest: String::new(),
        protocol_version: request.protocol_version,
        scope: request.scope,
        project_id: request.project_id,
        compiler_version: request.compiler_version,
        issued_at: request.issued_at,
        expires_at: request.expires_at,
        source: request.source,
        token_budget: ContextTokenBudget {
            requested: request.token_budget,
            used: used_tokens,
        },
        lifecycle: ContextPacketLifecycle::Compiled,
        selected,
        budget_excluded,
        rejected: request.rejected,
    }
    .seal()
    .map_err(ContextPacketCompilerError::from)
}

fn validate_candidates(
    mandatory: &[ContextPacketCandidate],
    optional: &[ContextPacketCandidate],
    rejected: &[ContextCandidateRejection],
    issued_at: i64,
) -> Result<(), ContextPacketCompilerError> {
    let mut seen = HashSet::new();
    let mut satisfied_requirements = HashSet::new();

    for candidate in mandatory {
        insert_candidate_id(&mut seen, &candidate.item_id)?;
        validate_candidate(candidate, true, issued_at)?;
        let Some(requirement) = candidate.reason.mandatory_requirement() else {
            return Err(
                ContextPacketCompilerError::MandatoryCandidateMissingRequirement {
                    item_id: candidate.item_id.clone(),
                },
            );
        };
        if !requirement.accepts(candidate.item_kind) {
            return Err(ContextPacketCompilerError::InvalidMandatoryKind {
                item_id: candidate.item_id.clone(),
                item_kind: candidate.item_kind,
                requirement,
            });
        }
        satisfied_requirements.insert(requirement);
    }

    for requirement in ContextMandatoryRequirement::ALL {
        if !satisfied_requirements.contains(&requirement) {
            return Err(ContextPacketCompilerError::MissingMandatoryRequirement { requirement });
        }
    }

    for candidate in optional {
        insert_candidate_id(&mut seen, &candidate.item_id)?;
        validate_candidate(candidate, false, issued_at)?;
    }

    for candidate in rejected {
        insert_candidate_id(&mut seen, &candidate.item_id)?;
        candidate.validate()?;
    }

    Ok(())
}

fn insert_candidate_id(
    seen: &mut HashSet<String>,
    item_id: &str,
) -> Result<(), ContextPacketCompilerError> {
    if seen.insert(item_id.to_string()) {
        Ok(())
    } else {
        Err(ContextPacketCompilerError::DuplicateCandidate {
            item_id: item_id.to_string(),
        })
    }
}

fn validate_candidate(
    candidate: &ContextPacketCandidate,
    mandatory: bool,
    issued_at: i64,
) -> Result<(), ContextPacketCompilerError> {
    if candidate.estimated_tokens == 0 {
        return Err(ContextPacketCompilerError::ZeroTokenEstimate {
            item_id: candidate.item_id.clone(),
        });
    }
    match candidate.freshness.expires_at {
        Some(expires_at) if expires_at <= issued_at => {
            return Err(ContextPacketCompilerError::StaleCandidate {
                item_id: candidate.item_id.clone(),
                issued_at,
                expires_at,
            });
        }
        _ => {}
    }
    selection_from(candidate.clone(), mandatory).validate()?;
    Ok(())
}

fn selection_from(candidate: ContextPacketCandidate, mandatory: bool) -> ContextSelection {
    ContextSelection {
        item_id: candidate.item_id,
        item_kind: candidate.item_kind,
        source_reference: candidate.source_reference,
        source_scope: candidate.source_scope,
        provenance: candidate.provenance,
        freshness: candidate.freshness,
        source_version: candidate.source_version,
        rendered: candidate.rendered,
        reason: candidate.reason,
        effective_relevance: (!mandatory).then_some(candidate.relevance),
        estimated_tokens: candidate.estimated_tokens,
        mandatory,
    }
}

#[cfg(test)]
mod tests {
    use ackplane_protocol::context_packet::{
        ContextCandidateRejectionReason, ContextFreshness, CONTEXT_PACKET_PROTOCOL_VERSION,
    };

    use super::*;

    fn scope() -> ContextPacketScope {
        ContextPacketScope {
            tenant_id: "tenant".to_string(),
            repository_id: "repository".to_string(),
            task_id: "task".to_string(),
            goal_id: "goal".to_string(),
            agent_session_id: "agent-session".to_string(),
        }
    }

    fn source() -> ContextPacketSource {
        ContextPacketSource {
            ledger_position: 7,
            projection_position: 5,
        }
    }

    fn source_scope() -> ContextItemScope {
        ContextItemScope {
            tenant_id: "tenant".to_string(),
            repository_id: "repository".to_string(),
            project_id: Some("project".to_string()),
            task_id: Some("task".to_string()),
            goal_id: Some("goal".to_string()),
        }
    }

    fn candidate(
        item_id: &str,
        item_kind: ContextItemKind,
        reason: ContextSelectionReason,
        estimated_tokens: u32,
        relevance: u64,
    ) -> ContextPacketCandidate {
        ContextPacketCandidate {
            item_id: item_id.to_string(),
            item_kind,
            source_reference: format!("source:{item_id}"),
            source_scope: source_scope(),
            provenance: ContextProvenance {
                recorded_by: "ackplane-projection".to_string(),
                recorded_at: 5,
                evidence_reference: Some(format!("evidence:{item_id}")),
            },
            freshness: ContextFreshness {
                observed_at: 5,
                expires_at: Some(30),
            },
            source_version: "v1".to_string(),
            rendered: format!("bounded rendering for {item_id}"),
            reason,
            estimated_tokens,
            relevance,
        }
    }

    fn required_candidates(estimated_tokens: u32) -> Vec<ContextPacketCandidate> {
        vec![
            candidate(
                "identity",
                ContextItemKind::TargetIdentity,
                ContextSelectionReason::RequiredTargetIdentity,
                estimated_tokens,
                0,
            ),
            candidate(
                "task-lease",
                ContextItemKind::TaskLease,
                ContextSelectionReason::RequiredTaskLease,
                estimated_tokens,
                0,
            ),
            candidate(
                "objective",
                ContextItemKind::Objective,
                ContextSelectionReason::RequiredObjective,
                estimated_tokens,
                0,
            ),
            candidate(
                "acceptance",
                ContextItemKind::Acceptance,
                ContextSelectionReason::RequiredAcceptance,
                estimated_tokens,
                0,
            ),
            candidate(
                "constitution",
                ContextItemKind::Constitution,
                ContextSelectionReason::RequiredConstitution,
                estimated_tokens,
                0,
            ),
            candidate(
                "policy",
                ContextItemKind::Policy,
                ContextSelectionReason::RequiredPolicy,
                estimated_tokens,
                0,
            ),
            candidate(
                "safety",
                ContextItemKind::SafetyControl,
                ContextSelectionReason::RequiredSafetyControl,
                estimated_tokens,
                0,
            ),
            candidate(
                "evidence-condition",
                ContextItemKind::EvidenceCondition,
                ContextSelectionReason::RequiredEvidenceCondition,
                estimated_tokens,
                0,
            ),
        ]
    }

    fn request(
        token_budget: u32,
        mandatory: Vec<ContextPacketCandidate>,
        optional: Vec<ContextPacketCandidate>,
    ) -> ContextPacketCompilationRequest {
        ContextPacketCompilationRequest {
            packet_id: "packet".to_string(),
            protocol_version: CONTEXT_PACKET_PROTOCOL_VERSION.to_string(),
            scope: scope(),
            project_id: Some("project".to_string()),
            compiler_version: "v2".to_string(),
            issued_at: 10,
            expires_at: 20,
            source: source(),
            token_budget,
            mandatory,
            optional,
            rejected: Vec::new(),
        }
    }

    #[test]
    fn mandatory_context_is_reserved_before_optional_context() {
        let packet = compile_context_packet(request(
            10,
            required_candidates(1),
            vec![candidate(
                "knowledge",
                ContextItemKind::Knowledge,
                ContextSelectionReason::WorkingSet,
                2,
                10,
            )],
        ))
        .expect("the complete mandatory envelope and one optional item fit");

        assert_eq!(
            packet
                .selected
                .iter()
                .map(|selection| selection.item_id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "acceptance",
                "constitution",
                "evidence-condition",
                "identity",
                "objective",
                "policy",
                "safety",
                "task-lease",
                "knowledge",
            ]
        );
        assert!(packet.selected[..7]
            .iter()
            .all(|selection| selection.mandatory));
        assert_eq!(packet.selected[8].effective_relevance, Some(10));
        assert_eq!(
            packet.token_budget,
            ContextTokenBudget {
                requested: 10,
                used: 10,
            }
        );
        assert!(packet.budget_excluded.is_empty());
        assert!(packet.rejected.is_empty());
        assert_eq!(packet.validate(), Ok(()));
    }

    #[test]
    fn a_missing_safety_control_is_refused_before_optional_ranking() {
        let mut mandatory = required_candidates(1);
        mandatory.retain(|candidate| candidate.item_id != "safety");

        let result = compile_context_packet(request(
            8,
            mandatory,
            vec![candidate(
                "knowledge",
                ContextItemKind::Knowledge,
                ContextSelectionReason::WorkingSet,
                1,
                10,
            )],
        ));

        assert!(matches!(
            result,
            Err(ContextPacketCompilerError::MissingMandatoryRequirement {
                requirement: ContextMandatoryRequirement::SafetyControl
            })
        ));
    }

    #[test]
    fn mandatory_context_that_exceeds_budget_is_refused() {
        let result = compile_context_packet(request(15, required_candidates(2), Vec::new()));

        assert!(matches!(
            result,
            Err(ContextPacketCompilerError::MandatoryBudgetExceeded {
                requested: 15,
                required: 16,
            })
        ));
    }

    #[test]
    fn optional_context_uses_relevance_then_identifier_and_never_splits_items() {
        let packet = compile_context_packet(request(
            11,
            required_candidates(1),
            vec![
                candidate(
                    "knowledge-z",
                    ContextItemKind::Knowledge,
                    ContextSelectionReason::WorkingSet,
                    3,
                    10,
                ),
                candidate(
                    "knowledge-a",
                    ContextItemKind::Knowledge,
                    ContextSelectionReason::WorkingSet,
                    3,
                    10,
                ),
                candidate(
                    "structural",
                    ContextItemKind::Structural,
                    ContextSelectionReason::GraphReach,
                    3,
                    9,
                ),
            ],
        ))
        .expect("one optional candidate should fit after the mandatory envelope");

        assert_eq!(
            packet
                .selected
                .iter()
                .map(|selection| selection.item_id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "acceptance",
                "constitution",
                "evidence-condition",
                "identity",
                "objective",
                "policy",
                "safety",
                "task-lease",
                "knowledge-a",
            ]
        );
        assert_eq!(
            packet
                .budget_excluded
                .iter()
                .map(|exclusion| {
                    (
                        exclusion.item_id.as_str(),
                        exclusion.ranking.effective_relevance,
                        exclusion.ranking.estimated_tokens,
                        exclusion.reason,
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                ("knowledge-z", 10, 3, ContextBudgetExclusionReason::Budget),
                ("structural", 9, 3, ContextBudgetExclusionReason::Budget),
            ]
        );
    }

    #[test]
    fn pre_budget_rejections_remain_distinct_from_budget_exclusions() {
        let mut request = request(8, required_candidates(1), Vec::new());
        request.rejected.push(ContextCandidateRejection {
            item_id: "knowledge:retired".to_string(),
            item_kind: ContextItemKind::Knowledge,
            source_reference: "knowledge:retired".to_string(),
            source_version: "v3".to_string(),
            reason: ContextCandidateRejectionReason::Retired,
        });

        let packet = compile_context_packet(request).expect("the mandatory envelope fits");

        assert!(packet.budget_excluded.is_empty());
        assert_eq!(
            packet.rejected,
            vec![ContextCandidateRejection {
                item_id: "knowledge:retired".to_string(),
                item_kind: ContextItemKind::Knowledge,
                source_reference: "knowledge:retired".to_string(),
                source_version: "v3".to_string(),
                reason: ContextCandidateRejectionReason::Retired,
            }]
        );
    }

    #[test]
    fn a_zero_token_candidate_is_refused_before_packet_construction() {
        let mut mandatory = required_candidates(1);
        mandatory[0].estimated_tokens = 0;

        let result = compile_context_packet(request(8, mandatory, Vec::new()));

        assert!(matches!(
            result,
            Err(ContextPacketCompilerError::ZeroTokenEstimate { item_id }) if item_id == "identity"
        ));
    }

    #[test]
    fn a_stale_candidate_is_refused_before_budget_selection() {
        let mut stale = candidate(
            "knowledge:stale",
            ContextItemKind::Knowledge,
            ContextSelectionReason::WorkingSet,
            1,
            10,
        );
        stale.freshness.expires_at = Some(10);

        let result = compile_context_packet(request(9, required_candidates(1), vec![stale]));

        assert!(matches!(
            result,
            Err(ContextPacketCompilerError::StaleCandidate {
                item_id,
                issued_at: 10,
                expires_at: 10,
            }) if item_id == "knowledge:stale"
        ));
    }

    #[test]
    fn an_optional_candidate_with_empty_rendered_content_is_refused() {
        let mut malformed = candidate(
            "knowledge",
            ContextItemKind::Knowledge,
            ContextSelectionReason::WorkingSet,
            1,
            1,
        );
        malformed.rendered.clear();

        let result = compile_context_packet(request(8, required_candidates(1), vec![malformed]));

        assert!(matches!(
            result,
            Err(ContextPacketCompilerError::Packet(
                ContextPacketError::EmptyField {
                    field: "selected.rendered"
                }
            ))
        ));
    }
}
