use crate::graph::test_support::{add_node, raw_edge, store, NOW};
use crate::model::{NodeType, RelationType};

const ROOT: &str = "C:/Users/dev/Repos/MindLeak-build";

fn reinforce(store: &crate::graph::GraphStore, src: &str, tgt: &str, times: usize) {
    for i in 0..times {
        store
            .upsert_edge(&raw_edge(
                src,
                tgt,
                RelationType::Modified,
                0.5,
                168.0,
                NOW + i as i64,
            ))
            .unwrap();
    }
}

fn edge_row(store: &crate::graph::GraphStore, src: &str, tgt: &str) -> Option<(f64, i64, i64)> {
    store
        .conn
        .query_row(
            "SELECT weight, reinforcement_count, first_seen FROM edges
             WHERE source_id = ?1 AND target_id = ?2",
            rusqlite::params![src, tgt],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok()
}

#[test]
fn an_absolute_node_collapses_onto_its_repo_relative_twin() {
    let s = store();
    let absolute = format!("artifact:{ROOT}/AGENTS.md");
    add_node(&s, &absolute, NodeType::Artifact, "AGENTS.md", NOW);
    add_node(
        &s,
        "artifact:AGENTS.md",
        NodeType::Artifact,
        "AGENTS.md",
        NOW,
    );

    let outcome = s.repair_workspace_paths(ROOT).unwrap();

    assert_eq!(outcome.nodes_rewritten, 1);
    assert_eq!(outcome.nodes_merged, 1);
    assert!(s.get_node(&absolute).unwrap().is_none());
    assert!(s.get_node("artifact:AGENTS.md").unwrap().is_some());
}

#[test]
fn merging_replays_reinforcement_instead_of_picking_a_winner() {
    // The whole point of the migration: a file split across two identities
    // earned reinforcement on both, and each half decays like a one-off
    // (ADR-0005). The survivor must carry the history both halves earned.
    let s = store();
    let absolute = format!("artifact:{ROOT}/src/lib.rs");
    add_node(&s, "execution:e1", NodeType::Execution, "cargo test", NOW);
    add_node(&s, &absolute, NodeType::Artifact, "src/lib.rs", NOW);
    add_node(
        &s,
        "artifact:src/lib.rs",
        NodeType::Artifact,
        "src/lib.rs",
        NOW,
    );

    reinforce(&s, "execution:e1", &absolute, 3);
    reinforce(&s, "execution:e1", "artifact:src/lib.rs", 2);

    let (split_weight, split_count, _) =
        edge_row(&s, "execution:e1", "artifact:src/lib.rs").unwrap();
    assert_eq!(split_count, 2);

    s.repair_workspace_paths(ROOT).unwrap();

    let (weight, count, _) = edge_row(&s, "execution:e1", "artifact:src/lib.rs").unwrap();
    // 2 + 3 reinforcements, not 2 and not 3.
    assert_eq!(count, 5);
    // 0.05 per reinforcement carried over, exactly the write-path rule.
    assert!(
        (weight - (split_weight + 0.15)).abs() < 1e-9,
        "weight {weight}"
    );
    assert!(edge_row(&s, "execution:e1", &absolute).is_none());
}

#[test]
fn merging_keeps_the_earliest_sighting() {
    // `first_seen` is a fact about the file, not about which spelling saw it.
    let s = store();
    let absolute = format!("artifact:{ROOT}/src/old.rs");
    add_node(&s, "execution:e1", NodeType::Execution, "run", NOW);
    add_node(&s, &absolute, NodeType::Artifact, "src/old.rs", NOW);
    add_node(
        &s,
        "artifact:src/old.rs",
        NodeType::Artifact,
        "src/old.rs",
        NOW,
    );

    s.upsert_edge(&raw_edge(
        "execution:e1",
        &absolute,
        RelationType::Modified,
        0.5,
        168.0,
        NOW - 5_000,
    ))
    .unwrap();
    s.upsert_edge(&raw_edge(
        "execution:e1",
        "artifact:src/old.rs",
        RelationType::Modified,
        0.5,
        168.0,
        NOW,
    ))
    .unwrap();

    s.repair_workspace_paths(ROOT).unwrap();

    let (_, _, first_seen) = edge_row(&s, "execution:e1", "artifact:src/old.rs").unwrap();
    assert_eq!(first_seen, NOW - 5_000);
}

#[test]
fn a_symbol_id_keeps_its_name_when_the_path_is_rewritten() {
    let s = store();
    let absolute = format!("symbol:{ROOT}/src/decay.rs:effective_weight");
    add_node(
        &s,
        &absolute,
        NodeType::Symbol,
        "effective_weight (fn)",
        NOW,
    );

    s.repair_workspace_paths(ROOT).unwrap();

    assert!(s
        .get_node("symbol:src/decay.rs:effective_weight")
        .unwrap()
        .is_some());
    assert!(s.get_node(&absolute).unwrap().is_none());
}

#[test]
fn repair_is_idempotent_and_leaves_foreign_paths_alone() {
    let s = store();
    let outside = "artifact:D:/elsewhere/other.rs";
    add_node(&s, outside, NodeType::Artifact, "other.rs", NOW);
    add_node(
        &s,
        &format!("artifact:{ROOT}/src/a.rs"),
        NodeType::Artifact,
        "src/a.rs",
        NOW,
    );

    let first = s.repair_workspace_paths(ROOT).unwrap();
    let second = s.repair_workspace_paths(ROOT).unwrap();

    assert_eq!(first.nodes_rewritten, 1);
    assert_eq!(second.nodes_rewritten, 0);
    // A path genuinely outside this checkout is not ours to rewrite.
    assert!(s.get_node(outside).unwrap().is_some());
}
