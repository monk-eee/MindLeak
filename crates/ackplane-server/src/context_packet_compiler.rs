//! Deterministic ContextPacket compilation (ADR-0114 decisions 1-4).
//!
//! This module compiles typed candidates into the protocol's immutable packet
//! shape. It neither stores packets nor authenticates a requester; later
//! service slices own those boundaries.

use std::collections::HashSet;

use ackplane_protocol::context_packet::{
    ContextExclusion, ContextExclusionReason, ContextItemKind, ContextPacket, ContextPacketError,
    ContextPacketScope, ContextPacketSource, ContextSelection, ContextSelectionReason,
    ContextTokenBudget,
};
use thiserror::Error;

/// One typed candidate the deterministic compiler may select into a packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPacketCandidate {
    pub item_id: String,
    pub item_kind: ContextItemKind,
    pub source_reference: String,
    pub source_version: String,
    pub reason: ContextSelectionReason,
    pub estimated_tokens: u32,
    pub relevance: u64,
}

/// Inputs required to compile one immutable packet without a model call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPacketCompilationRequest {
    pub packet_id: String,
    pub scope: ContextPacketScope,
    pub compiler_version: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub source: ContextPacketSource,
    pub token_budget: u32,
    pub mandatory: Vec<ContextPacketCandidate>,
    pub optional: Vec<ContextPacketCandidate>,
}

#[derive(Debug, Error)]
pub enum ContextPacketCompilerError {
    #[error("context packet candidate {item_id:?} must have a non-zero token estimate")]
    ZeroTokenEstimate { item_id: String },
    #[error("context packet candidate {item_id:?} must have a non-empty {field}")]
    InvalidCandidateField {
        item_id: String,
        field: &'static str,
    },
    #[error("context packet candidate {item_id:?} appears more than once")]
    DuplicateCandidate { item_id: String },
    #[error("mandatory candidate {item_id:?} is not governance, task, or evidence")]
    InvalidMandatoryKind { item_id: String },
    #[error("mandatory candidate {item_id:?} has an invalid selection reason")]
    InvalidMandatoryReason { item_id: String },
    #[error("mandatory candidates require {required} tokens but the packet budget is {requested}")]
    MandatoryBudgetExceeded { requested: u32, required: u32 },
    #[error("context packet token arithmetic overflowed")]
    TokenBudgetOverflow,
    #[error("compiled context packet is invalid: {0}")]
    Packet(#[from] ContextPacketError),
}

/// Compiles a packet by reserving all mandatory context before ranking optional context.
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

    validate_candidates(&request.mandatory, &request.optional)?;

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
    let mut excluded = Vec::new();

    for candidate in request.optional {
        let Some(next_total) = used_tokens.checked_add(candidate.estimated_tokens) else {
            return Err(ContextPacketCompilerError::TokenBudgetOverflow);
        };
        if next_total <= request.token_budget {
            used_tokens = next_total;
            selected.push(selection_from(candidate, false));
        } else {
            excluded.push(ContextExclusion {
                item_id: candidate.item_id,
                reason: ContextExclusionReason::Budget,
            });
        }
    }

    let packet = ContextPacket {
        packet_id: request.packet_id,
        scope: request.scope,
        compiler_version: request.compiler_version,
        issued_at: request.issued_at,
        expires_at: request.expires_at,
        source: request.source,
        token_budget: ContextTokenBudget {
            requested: request.token_budget,
            used: used_tokens,
        },
        selected,
        excluded,
    };
    packet.validate()?;
    Ok(packet)
}

fn validate_candidates(
    mandatory: &[ContextPacketCandidate],
    optional: &[ContextPacketCandidate],
) -> Result<(), ContextPacketCompilerError> {
    let mut seen = HashSet::new();
    for candidate in mandatory.iter().chain(optional) {
        for (field, value) in [
            ("item_id", candidate.item_id.as_str()),
            ("source_reference", candidate.source_reference.as_str()),
            ("source_version", candidate.source_version.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(ContextPacketCompilerError::InvalidCandidateField {
                    item_id: candidate.item_id.clone(),
                    field,
                });
            }
        }
        if candidate.estimated_tokens == 0 {
            return Err(ContextPacketCompilerError::ZeroTokenEstimate {
                item_id: candidate.item_id.clone(),
            });
        }
        if !seen.insert(candidate.item_id.as_str()) {
            return Err(ContextPacketCompilerError::DuplicateCandidate {
                item_id: candidate.item_id.clone(),
            });
        }
    }

    for candidate in mandatory {
        let valid_reason = matches!(
            (candidate.item_kind, candidate.reason),
            (
                ContextItemKind::Governance,
                ContextSelectionReason::RequiredGovernance
            ) | (
                ContextItemKind::Task,
                ContextSelectionReason::ExplicitTaskReference
            ) | (
                ContextItemKind::Evidence,
                ContextSelectionReason::EvidenceLink
            )
        );
        if !matches!(
            candidate.item_kind,
            ContextItemKind::Governance | ContextItemKind::Task | ContextItemKind::Evidence
        ) {
            return Err(ContextPacketCompilerError::InvalidMandatoryKind {
                item_id: candidate.item_id.clone(),
            });
        }
        if !valid_reason {
            return Err(ContextPacketCompilerError::InvalidMandatoryReason {
                item_id: candidate.item_id.clone(),
            });
        }
    }
    Ok(())
}

fn selection_from(candidate: ContextPacketCandidate, mandatory: bool) -> ContextSelection {
    ContextSelection {
        item_id: candidate.item_id,
        item_kind: candidate.item_kind,
        source_reference: candidate.source_reference,
        source_version: candidate.source_version,
        reason: candidate.reason,
        estimated_tokens: candidate.estimated_tokens,
        mandatory,
    }
}

#[cfg(test)]
mod tests {
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
            source_version: "v1".to_string(),
            reason,
            estimated_tokens,
            relevance,
        }
    }

    fn request(
        token_budget: u32,
        mandatory: Vec<ContextPacketCandidate>,
        optional: Vec<ContextPacketCandidate>,
    ) -> ContextPacketCompilationRequest {
        ContextPacketCompilationRequest {
            packet_id: "packet".to_string(),
            scope: scope(),
            compiler_version: "v1".to_string(),
            issued_at: 10,
            expires_at: 20,
            source: source(),
            token_budget,
            mandatory,
            optional,
        }
    }

    #[test]
    fn mandatory_context_is_reserved_before_optional_context() {
        let packet = compile_context_packet(request(
            9,
            vec![
                candidate(
                    "governance",
                    ContextItemKind::Governance,
                    ContextSelectionReason::RequiredGovernance,
                    2,
                    0,
                ),
                candidate(
                    "task",
                    ContextItemKind::Task,
                    ContextSelectionReason::ExplicitTaskReference,
                    3,
                    0,
                ),
                candidate(
                    "evidence",
                    ContextItemKind::Evidence,
                    ContextSelectionReason::EvidenceLink,
                    2,
                    0,
                ),
            ],
            vec![candidate(
                "knowledge",
                ContextItemKind::Knowledge,
                ContextSelectionReason::WorkingSet,
                2,
                10,
            )],
        ))
        .expect("mandatory context and one optional candidate should fit");

        assert_eq!(
            packet.selected,
            vec![
                ContextSelection {
                    item_id: "evidence".to_string(),
                    item_kind: ContextItemKind::Evidence,
                    source_reference: "source:evidence".to_string(),
                    source_version: "v1".to_string(),
                    reason: ContextSelectionReason::EvidenceLink,
                    estimated_tokens: 2,
                    mandatory: true,
                },
                ContextSelection {
                    item_id: "governance".to_string(),
                    item_kind: ContextItemKind::Governance,
                    source_reference: "source:governance".to_string(),
                    source_version: "v1".to_string(),
                    reason: ContextSelectionReason::RequiredGovernance,
                    estimated_tokens: 2,
                    mandatory: true,
                },
                ContextSelection {
                    item_id: "task".to_string(),
                    item_kind: ContextItemKind::Task,
                    source_reference: "source:task".to_string(),
                    source_version: "v1".to_string(),
                    reason: ContextSelectionReason::ExplicitTaskReference,
                    estimated_tokens: 3,
                    mandatory: true,
                },
                ContextSelection {
                    item_id: "knowledge".to_string(),
                    item_kind: ContextItemKind::Knowledge,
                    source_reference: "source:knowledge".to_string(),
                    source_version: "v1".to_string(),
                    reason: ContextSelectionReason::WorkingSet,
                    estimated_tokens: 2,
                    mandatory: false,
                },
            ]
        );
        assert_eq!(
            packet.token_budget,
            ContextTokenBudget {
                requested: 9,
                used: 9,
            }
        );
        assert_eq!(packet.excluded, Vec::<ContextExclusion>::new());
    }

    #[test]
    fn mandatory_context_that_exceeds_budget_is_refused() {
        let result = compile_context_packet(request(
            5,
            vec![
                candidate(
                    "governance",
                    ContextItemKind::Governance,
                    ContextSelectionReason::RequiredGovernance,
                    2,
                    0,
                ),
                candidate(
                    "task",
                    ContextItemKind::Task,
                    ContextSelectionReason::ExplicitTaskReference,
                    2,
                    0,
                ),
                candidate(
                    "evidence",
                    ContextItemKind::Evidence,
                    ContextSelectionReason::EvidenceLink,
                    2,
                    0,
                ),
            ],
            Vec::new(),
        ));

        assert!(matches!(
            result,
            Err(ContextPacketCompilerError::MandatoryBudgetExceeded {
                requested: 5,
                required: 6,
            })
        ));
    }

    #[test]
    fn optional_context_uses_relevance_then_identifier_and_never_splits_items() {
        let packet = compile_context_packet(request(
            5,
            vec![candidate(
                "governance",
                ContextItemKind::Governance,
                ContextSelectionReason::RequiredGovernance,
                2,
                0,
            )],
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
        .expect("one optional candidate should fit after mandatory context");

        assert_eq!(
            packet
                .selected
                .iter()
                .map(|selection| selection.item_id.as_str())
                .collect::<Vec<_>>(),
            vec!["governance", "knowledge-a"]
        );
        assert_eq!(
            packet.excluded,
            vec![
                ContextExclusion {
                    item_id: "knowledge-z".to_string(),
                    reason: ContextExclusionReason::Budget,
                },
                ContextExclusion {
                    item_id: "structural".to_string(),
                    reason: ContextExclusionReason::Budget,
                },
            ]
        );
    }

    #[test]
    fn a_zero_token_candidate_is_refused_before_packet_construction() {
        let result = compile_context_packet(request(
            10,
            vec![candidate(
                "governance",
                ContextItemKind::Governance,
                ContextSelectionReason::RequiredGovernance,
                0,
                0,
            )],
            Vec::new(),
        ));

        assert!(matches!(
            result,
            Err(ContextPacketCompilerError::ZeroTokenEstimate { item_id }) if item_id == "governance"
        ));
    }

    #[test]
    fn an_over_budget_optional_candidate_with_empty_source_metadata_is_refused() {
        let mut malformed = candidate(
            "knowledge",
            ContextItemKind::Knowledge,
            ContextSelectionReason::WorkingSet,
            8,
            1,
        );
        malformed.source_reference.clear();

        let result = compile_context_packet(request(
            2,
            vec![candidate(
                "governance",
                ContextItemKind::Governance,
                ContextSelectionReason::RequiredGovernance,
                2,
                0,
            )],
            vec![malformed],
        ));

        assert!(matches!(
            result,
            Err(ContextPacketCompilerError::InvalidCandidateField {
                item_id,
                field: "source_reference",
            }) if item_id == "knowledge"
        ));
    }
}
