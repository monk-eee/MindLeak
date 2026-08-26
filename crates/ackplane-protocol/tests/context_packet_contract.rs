use ackplane_protocol::context_packet::{
    ContextBudgetExclusion, ContextBudgetExclusionReason, ContextCandidateRejection,
    ContextCandidateRejectionReason, ContextFreshness, ContextItemKind, ContextItemScope,
    ContextPacket, ContextPacketError, ContextPacketLifecycle, ContextPacketScope,
    ContextPacketSource, ContextPacketUseReceipt, ContextPacketUseStatus, ContextProvenance,
    ContextRankingInputs, ContextSelection, ContextSelectionReason, ContextTokenBudget,
    CONTEXT_PACKET_PROTOCOL_VERSION,
};

fn scope() -> ContextPacketScope {
    ContextPacketScope {
        tenant_id: "tenant-a".to_string(),
        repository_id: "repository-a".to_string(),
        task_id: "task:a".to_string(),
        goal_id: "goal:a".to_string(),
        agent_session_id: "session:v1:agent-a".to_string(),
    }
}

fn source_scope() -> ContextItemScope {
    ContextItemScope {
        tenant_id: "tenant-a".to_string(),
        repository_id: "repository-a".to_string(),
        project_id: Some("project-a".to_string()),
        task_id: Some("task:a".to_string()),
        goal_id: Some("goal:a".to_string()),
    }
}

fn selection(
    item_id: &str,
    item_kind: ContextItemKind,
    reason: ContextSelectionReason,
) -> ContextSelection {
    ContextSelection {
        item_id: item_id.to_string(),
        item_kind,
        source_reference: format!("source:{item_id}"),
        source_scope: source_scope(),
        provenance: ContextProvenance {
            recorded_by: "constitution-projection".to_string(),
            recorded_at: 1_700_000_000,
            evidence_reference: Some("ledger:42".to_string()),
        },
        freshness: ContextFreshness {
            observed_at: 1_700_000_000,
            expires_at: Some(1_700_000_300),
        },
        source_version: "4".to_string(),
        rendered: format!("Bounded rendered context for {item_id}."),
        reason,
        effective_relevance: None,
        estimated_tokens: 48,
        mandatory: true,
    }
}

fn mandatory_selections() -> Vec<ContextSelection> {
    vec![
        selection(
            "identity",
            ContextItemKind::TargetIdentity,
            ContextSelectionReason::RequiredTargetIdentity,
        ),
        selection(
            "task-lease",
            ContextItemKind::TaskLease,
            ContextSelectionReason::RequiredTaskLease,
        ),
        selection(
            "objective",
            ContextItemKind::Objective,
            ContextSelectionReason::RequiredObjective,
        ),
        selection(
            "acceptance",
            ContextItemKind::Acceptance,
            ContextSelectionReason::RequiredAcceptance,
        ),
        selection(
            "constitution:v4",
            ContextItemKind::Constitution,
            ContextSelectionReason::RequiredConstitution,
        ),
        selection(
            "policy",
            ContextItemKind::Policy,
            ContextSelectionReason::RequiredPolicy,
        ),
        selection(
            "safety",
            ContextItemKind::SafetyControl,
            ContextSelectionReason::RequiredSafetyControl,
        ),
        selection(
            "evidence-condition",
            ContextItemKind::EvidenceCondition,
            ContextSelectionReason::RequiredEvidenceCondition,
        ),
    ]
}

fn packet() -> ContextPacket {
    ContextPacket {
        packet_id: "context:a".to_string(),
        digest: String::new(),
        protocol_version: CONTEXT_PACKET_PROTOCOL_VERSION.to_string(),
        scope: scope(),
        project_id: Some("project-a".to_string()),
        compiler_version: "v2".to_string(),
        issued_at: 1_700_000_000,
        expires_at: 1_700_000_300,
        source: ContextPacketSource {
            ledger_position: 42,
            projection_position: 40,
        },
        token_budget: ContextTokenBudget {
            requested: 1_024,
            used: 384,
        },
        lifecycle: ContextPacketLifecycle::Compiled,
        selected: mandatory_selections(),
        budget_excluded: vec![ContextBudgetExclusion {
            item_id: "knowledge:low-rank".to_string(),
            item_kind: ContextItemKind::Knowledge,
            ranking: ContextRankingInputs {
                effective_relevance: 1,
                estimated_tokens: 64,
                stable_tie_breaker: "knowledge:low-rank".to_string(),
            },
            reason: ContextBudgetExclusionReason::Budget,
        }],
        rejected: vec![ContextCandidateRejection {
            item_id: "knowledge:retired".to_string(),
            item_kind: ContextItemKind::Knowledge,
            source_reference: "knowledge:retired".to_string(),
            source_version: "3".to_string(),
            reason: ContextCandidateRejectionReason::Retired,
        }],
    }
    .seal()
    .expect("fixture packet must be valid")
}

#[test]
fn valid_packet_round_trips_through_json_and_validates() {
    let packet = packet();

    assert_eq!(packet.validate(), Ok(()));
    assert_eq!(packet.digest.len(), 64);

    let serialized = serde_json::to_string(&packet).expect("serialize packet");
    let decoded: ContextPacket = serde_json::from_str(&serialized).expect("deserialize packet");

    assert_eq!(decoded, packet);
}

#[test]
fn packet_without_the_complete_mandatory_envelope_is_refused() {
    let mut packet = packet();
    packet
        .selected
        .retain(|selection| selection.item_id != "task-lease");

    assert_eq!(
        packet.validate(),
        Err(ContextPacketError::MissingMandatoryEnvelopeRequirement {
            requirement: ackplane_protocol::context_packet::ContextMandatoryRequirement::TaskLease,
        })
    );
}

#[test]
fn blank_scope_identity_is_refused() {
    let mut packet = packet();
    packet.scope.tenant_id = " ".to_string();

    assert_eq!(
        packet.validate(),
        Err(ContextPacketError::EmptyField { field: "tenant_id" })
    );
}

#[test]
fn expiry_at_issuance_is_refused() {
    let mut packet = packet();
    packet.expires_at = packet.issued_at;

    assert_eq!(
        packet.validate(),
        Err(ContextPacketError::ExpiryMustFollowIssuance)
    );
}

#[test]
fn token_use_above_budget_is_refused() {
    let mut packet = packet();
    packet.token_budget = ContextTokenBudget {
        requested: 384,
        used: 385,
    };

    assert_eq!(
        packet.validate(),
        Err(ContextPacketError::TokenBudgetExceeded {
            requested: 384,
            used: 385,
        })
    );
}

#[test]
fn duplicate_selected_item_is_refused() {
    let mut packet = packet();
    packet.selected.push(selection(
        "constitution:v4",
        ContextItemKind::Constitution,
        ContextSelectionReason::RequiredConstitution,
    ));

    assert_eq!(
        packet.validate(),
        Err(ContextPacketError::DuplicateSelectedItem {
            item_id: "constitution:v4".to_string(),
        })
    );
}

#[test]
fn selected_item_cannot_also_be_budget_excluded() {
    let mut packet = packet();
    packet.budget_excluded = vec![ContextBudgetExclusion {
        item_id: "constitution:v4".to_string(),
        item_kind: ContextItemKind::Constitution,
        ranking: ContextRankingInputs {
            effective_relevance: 1,
            estimated_tokens: 64,
            stable_tie_breaker: "constitution:v4".to_string(),
        },
        reason: ContextBudgetExclusionReason::Budget,
    }];

    assert_eq!(
        packet.validate(),
        Err(ContextPacketError::DuplicatePacketItem {
            item_id: "constitution:v4".to_string(),
        })
    );
}

#[test]
fn a_tampered_packet_fails_its_content_digest() {
    let mut packet = packet();
    packet.compiler_version = "v3".to_string();

    assert!(matches!(
        packet.validate(),
        Err(ContextPacketError::DigestMismatch { .. })
    ));
}

#[test]
fn packet_use_receipt_round_trips_and_validates_its_scope() {
    let receipt = ContextPacketUseReceipt {
        packet_id: "context:a".to_string(),
        scope: scope(),
        occurred_at: 1_700_000_030,
        status: ContextPacketUseStatus::AppliedToPlanning,
        reason: None,
    };

    assert_eq!(receipt.validate(), Ok(()));

    let serialized = serde_json::to_string(&receipt).expect("serialize receipt");
    let decoded: ContextPacketUseReceipt =
        serde_json::from_str(&serialized).expect("deserialize receipt");

    assert_eq!(decoded, receipt);
}
