use crate::graph::{Direction, GraphStore};
use crate::ingest::git::{ingest_commit, CommitRecord};
use crate::{db, telemetry, Edge, Node, NodeType, RelationType};

fn polluted_commit() -> (GraphStore, CommitRecord) {
    let store = GraphStore::new(db::open_in_memory().unwrap());
    let verified = CommitRecord {
        sha: Some("1".repeat(40)),
        message: "fix: exact facts\n\nWHY: only this commit".into(),
        changed_files: vec!["src/keep.rs".into(), "src/restored.rs".into()],
        timestamp: 100,
    };
    let wrong = CommitRecord {
        message: "wrong publication description".into(),
        changed_files: vec!["src/keep.rs".into(), "src/false.rs".into()],
        timestamp: 900,
        ..verified.clone()
    };
    ingest_commit(&store, &wrong, 950, &[], None).unwrap();
    let peer = CommitRecord {
        sha: Some("2".repeat(40)),
        message: "peer's actual change".into(),
        changed_files: vec!["src/false.rs".into()],
        timestamp: 80,
    };
    ingest_commit(&store, &peer, 950, &[], None).unwrap();
    store
        .upsert_node(&Node::new("agent:reader", NodeType::Agent, "reader", 901))
        .unwrap();
    store
        .upsert_edge(&Edge::new(
            "agent:reader",
            "artifact:src/false.rs",
            RelationType::Observed,
            901,
        ))
        .unwrap();
    store
        .upsert_edge(&Edge::new(
            format!("intent:{}", "1".repeat(40)),
            "artifact:src/false.rs",
            RelationType::RelatesTo,
            901,
        ))
        .unwrap();
    (store, verified)
}

// Correct replay previously left branch-wide refactored edges behind forever.
// Explicit repair must retract only those false facts and preserve their audit.
#[test]
fn commit_repair_preserves_unrelated_history_and_audits_the_previous_facts() {
    let (store, verified) = polluted_commit();
    let intent = format!("intent:{}", "1".repeat(40));
    let peer = format!("intent:{}", "2".repeat(40));
    let unrelated_before = store
        .traverse(
            &[peer.clone(), "agent:reader".into()],
            Direction::Outgoing,
            1,
            0.0,
            1000,
        )
        .unwrap();
    let artifact_before = store.get_node("artifact:src/false.rs").unwrap();

    let outcome = store
        .repair_commit_attribution(
            "repairer",
            &verified,
            "branch delta was attributed to one commit",
            1000,
            &[],
        )
        .unwrap();

    assert_eq!(outcome.removed_edges, 1);
    assert_eq!(outcome.added_edges, 1);
    assert!(outcome.metadata_changed);
    assert!(outcome.audit_id.is_some());
    let repaired = store
        .traverse(
            std::slice::from_ref(&intent),
            Direction::Outgoing,
            1,
            0.0,
            1000,
        )
        .unwrap();
    let mut targets: Vec<_> = repaired
        .edges
        .iter()
        .filter(|edge| edge.relation == RelationType::Refactored)
        .map(|edge| edge.target_id.as_str())
        .collect();
    targets.sort();
    assert_eq!(
        targets,
        ["artifact:src/keep.rs", "artifact:src/restored.rs"]
    );
    assert!(repaired
        .edges
        .iter()
        .any(|edge| edge.relation == RelationType::RelatesTo));
    assert_eq!(store.get_node(&intent).unwrap().unwrap().created_at, 100);
    assert_eq!(
        serde_json::to_value(store.get_node("artifact:src/false.rs").unwrap()).unwrap(),
        serde_json::to_value(artifact_before).unwrap()
    );
    let unrelated_after = store
        .traverse(
            &[peer, "agent:reader".into()],
            Direction::Outgoing,
            1,
            0.0,
            1000,
        )
        .unwrap();
    let mut before_snapshot = serde_json::to_value(unrelated_before).unwrap();
    let mut after_snapshot = serde_json::to_value(unrelated_after).unwrap();
    for snapshot in [&mut before_snapshot, &mut after_snapshot] {
        snapshot["nodes"]
            .as_array_mut()
            .unwrap()
            .sort_by_key(|node| node["id"].as_str().unwrap().to_owned());
    }
    assert_eq!(before_snapshot, after_snapshot);
    let audit = telemetry::snapshot(&store.conn, 10)
        .unwrap()
        .recent
        .into_iter()
        .find(|event| event.kind == "evidence_repair")
        .unwrap();
    let detail = audit.detail.unwrap();
    assert_eq!(detail["agent_id"], "repairer");
    assert_eq!(detail["before"]["node"]["created_at"], 900);
    assert_eq!(detail["before"]["edges"].as_array().unwrap().len(), 2);
    assert_eq!(detail["after"]["node"]["created_at"], 100);
}

#[test]
fn repeating_commit_repair_does_not_reinforce_edges_or_duplicate_the_audit() {
    let (store, verified) = polluted_commit();
    let first = store
        .repair_commit_attribution("repairer", &verified, "correct attribution", 1000, &[])
        .unwrap();
    let second = store
        .repair_commit_attribution("repairer", &verified, "retry", 1100, &[])
        .unwrap();

    assert!(first.audit_id.is_some());
    assert!(second.audit_id.is_none());
    assert_eq!(
        second.removed_edges + second.added_edges + second.corrected_timestamps,
        0
    );
    assert!(!second.metadata_changed);
    assert_eq!(
        telemetry::snapshot(&store.conn, 10).unwrap().total_events,
        1
    );
    let facts: (i64, i64, i64) = store
        .conn
        .query_row(
            "SELECT updated_at, first_seen, reinforcement_count FROM edges
         WHERE source_id = ?1 AND target_id = 'artifact:src/keep.rs' AND relation = 'refactored'",
            [format!("intent:{}", "1".repeat(40))],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(facts, (100, 100, 1));
}

// A repair without its audit would silently destroy historical evidence.
// A failing audit insert must roll back removals, additions and metadata.
#[test]
fn an_unwritable_audit_rolls_back_the_entire_commit_repair() {
    let (store, verified) = polluted_commit();
    telemetry::ensure_table(&store.conn).unwrap();
    store
        .conn
        .execute_batch(
            "CREATE TRIGGER refuse_repair_audit BEFORE INSERT ON telemetry_events
         WHEN NEW.kind = 'evidence_repair'
         BEGIN SELECT RAISE(ABORT, 'audit unavailable'); END;",
        )
        .unwrap();
    let error = store
        .repair_commit_attribution("repairer", &verified, "correct attribution", 1000, &[])
        .unwrap_err();
    assert!(error.to_string().contains("audit unavailable"));
    let intent = format!("intent:{}", "1".repeat(40));
    let graph = store
        .traverse(
            std::slice::from_ref(&intent),
            Direction::Outgoing,
            1,
            0.0,
            1000,
        )
        .unwrap();
    assert!(graph
        .edges
        .iter()
        .any(|edge| edge.relation == RelationType::Refactored
            && edge.target_id == "artifact:src/false.rs"));
    assert_eq!(store.get_node(&intent).unwrap().unwrap().created_at, 900);
    assert!(store
        .get_node("artifact:src/restored.rs")
        .unwrap()
        .is_none());
    assert_eq!(
        telemetry::snapshot(&store.conn, 10).unwrap().total_events,
        0
    );
}

#[test]
fn a_verified_empty_delta_removes_only_the_target_commits_false_edges() {
    let (store, mut verified) = polluted_commit();
    verified.changed_files.clear();
    let outcome = store
        .repair_commit_attribution(
            "repairer",
            &verified,
            "this clean merge authored no files",
            1000,
            &[],
        )
        .unwrap();
    assert_eq!(outcome.removed_edges, 2);
    assert_eq!(outcome.added_edges, 0);
    let target = store
        .traverse(&[outcome.intent_id], Direction::Outgoing, 1, 0.0, 1000)
        .unwrap();
    assert!(target
        .edges
        .iter()
        .all(|edge| edge.relation != RelationType::Refactored));
    let peer = store
        .traverse(
            &[format!("intent:{}", "2".repeat(40))],
            Direction::Outgoing,
            1,
            0.0,
            1000,
        )
        .unwrap();
    assert_eq!(peer.edges.len(), 1);
    assert_eq!(peer.edges[0].target_id, "artifact:src/false.rs");
    assert!(outcome.audit_id.is_some());
}
