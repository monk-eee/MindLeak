use mindleak_core::decay::{signal_multiplier, SignalEvidence, PRUNE_THRESHOLD};
use mindleak_core::{db, Direction, Edge, GraphStore, Node, NodeType, RelationType};
use serde_json::json;

const NOW: i64 = 2_000_000_000;
const HOUR: i64 = 3600;

fn edge(source: &str, target: &str, relation: RelationType, updated_at: i64) -> Edge {
    Edge::new(source, target, relation, updated_at)
}

fn main() -> mindleak_core::Result<()> {
    let store = GraphStore::new(db::open_in_memory()?);
    let stable = Node::new(
        "artifact:stable",
        NodeType::Artifact,
        "stable",
        NOW - 10 * 24 * HOUR,
    );
    let spam_target = Node::new("artifact:spam", NodeType::Artifact, "spam", NOW);
    let consumer = Node::new("artifact:consumer", NodeType::Artifact, "consumer", NOW);
    let failure = Node::new("execution:failure", NodeType::Execution, "cargo test", NOW)
        .with_content("exit=1\n");
    let success = Node::new(
        "execution:success",
        NodeType::Execution,
        "cargo test",
        NOW + 2 * HOUR,
    )
    .with_content("exit=0\n");
    let spam =
        Node::new("execution:spam", NodeType::Execution, "green", NOW).with_content("exit=0\n");
    let commit = Node::new("intent:commit", NodeType::Intent, "fix stable", NOW + HOUR);
    let decision = Node::new(
        "intent:decision",
        NodeType::Intent,
        "DECISION: preserve stable",
        NOW + HOUR,
    );
    for node in [
        stable,
        spam_target,
        consumer,
        failure,
        success,
        spam,
        commit,
        decision,
    ] {
        store.upsert_node(&node)?;
    }

    let failed_on = edge(
        "execution:failure",
        "artifact:stable",
        RelationType::FailedOn,
        NOW,
    );
    store.upsert_edge(&failed_on)?;
    store.upsert_edge(&edge(
        "intent:commit",
        "artifact:stable",
        RelationType::Refactored,
        NOW + HOUR,
    ))?;
    store.upsert_edge(&edge(
        "artifact:consumer",
        "artifact:stable",
        RelationType::Imports,
        NOW,
    ))?;
    store.upsert_edge(&edge(
        "intent:decision",
        "artifact:stable",
        RelationType::RelatesTo,
        NOW + HOUR,
    ))?;
    for _ in 0..400 {
        store.upsert_edge(&edge(
            "execution:spam",
            "artifact:spam",
            RelationType::Modified,
            NOW,
        ))?;
    }

    let at_six_days = NOW + 6 * 24 * HOUR;
    let resolved = store
        .traverse(
            &["artifact:stable".to_string()],
            Direction::Incoming,
            1,
            0.0,
            at_six_days,
        )?
        .edges
        .into_iter()
        .find(|candidate| candidate.relation == RelationType::FailedOn)
        .expect("resolved failure edge");
    let spammed = store
        .traverse(
            &["artifact:spam".to_string()],
            Direction::Incoming,
            1,
            0.0,
            at_six_days,
        )?
        .edges
        .into_iter()
        .find(|candidate| candidate.relation == RelationType::Modified)
        .expect("spammed modified edge");
    let at_sixty_days = store
        .traverse(
            &["artifact:stable".to_string()],
            Direction::Incoming,
            1,
            0.0,
            NOW + 60 * 24 * HOUR,
        )?
        .edges
        .into_iter()
        .find(|candidate| candidate.relation == RelationType::FailedOn)
        .expect("eventually decayed failure edge");
    let handoff = store.prune_with_signal(NOW + 60 * 24 * HOUR)?;
    let failure_retained = store.get_node("execution:failure")?.is_some();

    let baseline = SignalEvidence::default();
    let reinforcement = SignalEvidence {
        reinforcement_count: 12,
        reinforcement_span_hours: 96.0,
        ..baseline
    };
    let diversity = SignalEvidence {
        source_diversity: 4,
        ..baseline
    };
    let consequence = SignalEvidence {
        consequence: true,
        ..baseline
    };
    let surprise = SignalEvidence {
        surprise: true,
        ..baseline
    };
    let centrality = SignalEvidence {
        structural_in_degree: 16,
        ..baseline
    };
    let attention = SignalEvidence {
        deliberate_attention: true,
        ..baseline
    };
    let maximal = SignalEvidence {
        reinforcement_count: 10_000,
        reinforcement_span_hours: 10_000.0,
        source_diversity: 100,
        consequence: true,
        surprise: true,
        structural_in_degree: 10_000,
        deliberate_attention: true,
    };

    let result = json!({
        "schema_version": 1,
        "threshold": PRUNE_THRESHOLD,
        "scenarios": {
            "same_session_spam": {
                "reinforcement_count": 400,
                "signal_multiplier": spammed.signal_multiplier,
                "effective_after_six_days": spammed.effective,
                "active": spammed.effective >= PRUNE_THRESHOLD
            },
            "resolved_failure": {
                "signal_multiplier": resolved.signal_multiplier,
                "effective_after_six_days": resolved.effective,
                "active": resolved.effective >= PRUNE_THRESHOLD,
                "evidence": resolved.signal_evidence
            },
            "eventual_decay": {
                "effective_after_sixty_days": at_sixty_days.effective,
                "active": at_sixty_days.effective >= PRUNE_THRESHOLD
            },
            "consolidation_handoff": {
                "candidate_count": handoff.signal_candidates.len(),
                "contains_failure": handoff.signal_candidates.iter().any(|candidate| candidate.relation == RelationType::FailedOn),
                "expired_failure_retained": failure_retained
            }
        },
        "ablation": {
            "baseline": signal_multiplier(baseline),
            "span_qualified_reinforcement": signal_multiplier(reinforcement),
            "source_diversity": signal_multiplier(diversity),
            "consequence": signal_multiplier(consequence),
            "surprise": signal_multiplier(surprise),
            "structural_centrality": signal_multiplier(centrality),
            "deliberate_attention": signal_multiplier(attention),
            "maximal": signal_multiplier(maximal)
        }
    });
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
