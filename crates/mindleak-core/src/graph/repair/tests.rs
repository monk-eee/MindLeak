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

/// Regression: repair was prefix-scoped, which assumes every worktree
/// eventually hosts a server that heals its own ids. A worktree an agent works
/// in without ever starting a server there leaves its ids orphaned permanently,
/// and because the absolute id owns the structural edges, the repo-relative id
/// can never take them: `ingest_file` fails with "structural edge is owned by
/// artifact:<other checkout>/x, not artifact:x". Measured 2026-07-29, 43 of 247
/// tracked files were stuck this way and every future extractor improvement
/// would have missed them silently.
#[test]
fn a_sibling_checkouts_absolute_id_collapses_onto_the_twin_the_graph_already_has() {
    let s = store();
    let sibling = "artifact:C:/Users/dev/Repos/MindLeak-export/scripts/design-audit.mjs";
    add_node(&s, sibling, NodeType::Artifact, "design-audit.mjs", NOW);
    add_node(
        &s,
        "artifact:scripts/design-audit.mjs",
        NodeType::Artifact,
        "scripts/design-audit.mjs",
        NOW,
    );

    // ROOT is this checkout; the sibling is spelled under a different one.
    let outcome = s.repair_workspace_paths(ROOT).unwrap();

    assert_eq!(
        outcome.nodes_merged, 1,
        "the duplicate identity is collapsed"
    );
    assert!(
        s.get_node(sibling).unwrap().is_none(),
        "the sibling's absolute id is retired"
    );
    assert!(s
        .get_node("artifact:scripts/design-audit.mjs")
        .unwrap()
        .is_some());
}

/// The warrant is that the twin was already observed, not that the path looked
/// absolute. Without a twin the id stays exactly where it is — inventing a
/// relative form for something genuinely elsewhere would invent a file that
/// does not exist, which is what the prefix pass was protecting.
#[test]
fn an_absolute_path_with_no_twin_in_the_graph_is_still_left_alone() {
    let s = store();
    let elsewhere = "artifact:D:/some/other/project/main.rs";
    add_node(&s, elsewhere, NodeType::Artifact, "main.rs", NOW);

    let outcome = s.repair_workspace_paths(ROOT).unwrap();

    assert_eq!(outcome.nodes_rewritten, 0);
    assert!(s.get_node(elsewhere).unwrap().is_some());
    // Nor was a relative id conjured for it.
    assert!(s.get_node("artifact:main.rs").unwrap().is_none());
    assert!(s
        .get_node("artifact:some/other/project/main.rs")
        .unwrap()
        .is_none());
}

/// A bare filename can collide across directories, so the longest suffix the
/// graph already holds wins. Matching `util.rs` when
/// `crates/a/src/util.rs` is present would merge two different files into one.
#[test]
fn the_longest_known_suffix_wins_over_a_bare_filename() {
    let s = store();
    let absolute = "artifact:C:/Users/dev/Repos/MindLeak-other/crates/a/src/util.rs";
    add_node(&s, absolute, NodeType::Artifact, "util.rs", NOW);
    add_node(&s, "artifact:util.rs", NodeType::Artifact, "util.rs", NOW);
    add_node(
        &s,
        "artifact:crates/a/src/util.rs",
        NodeType::Artifact,
        "crates/a/src/util.rs",
        NOW,
    );

    s.repair_workspace_paths(ROOT).unwrap();

    assert!(s.get_node(absolute).unwrap().is_none());
    assert!(s
        .get_node("artifact:crates/a/src/util.rs")
        .unwrap()
        .is_some());
    // The top-level file is a different file and keeps its own identity.
    assert!(s.get_node("artifact:util.rs").unwrap().is_some());
}

/// A symbol id is `symbol:<path>:<name>`; only the path half is a duplicate
/// identity, and the name must survive the collapse intact.
#[test]
fn a_sibling_symbol_id_collapses_and_keeps_its_name() {
    let s = store();
    let absolute = "symbol:C:/Users/dev/Repos/MindLeak-export/src/decay.rs:effective_weight";
    add_node(&s, absolute, NodeType::Symbol, "effective_weight", NOW);
    add_node(
        &s,
        "symbol:src/decay.rs:effective_weight",
        NodeType::Symbol,
        "effective_weight",
        NOW,
    );

    s.repair_workspace_paths(ROOT).unwrap();

    assert!(s.get_node(absolute).unwrap().is_none());
    assert!(s
        .get_node("symbol:src/decay.rs:effective_weight")
        .unwrap()
        .is_some());
}

/// The collapse carries the edges with it, which is the entire point: the
/// absolute id owning the structural edges is what blocked re-ingest.
#[test]
fn collapsing_a_sibling_id_carries_its_edges_onto_the_twin() {
    let s = store();
    let sibling = "artifact:C:/Users/dev/Repos/MindLeak-export/scripts/a.mjs";
    add_node(&s, sibling, NodeType::Artifact, "a.mjs", NOW);
    add_node(
        &s,
        "artifact:scripts/a.mjs",
        NodeType::Artifact,
        "scripts/a.mjs",
        NOW,
    );
    add_node(
        &s,
        "artifact:scripts/b.mjs",
        NodeType::Artifact,
        "scripts/b.mjs",
        NOW,
    );
    reinforce(&s, sibling, "artifact:scripts/b.mjs", 3);

    s.repair_workspace_paths(ROOT).unwrap();

    assert!(
        edge_row(&s, "artifact:scripts/a.mjs", "artifact:scripts/b.mjs").is_some(),
        "the edge must survive under the surviving id"
    );
    assert!(edge_row(&s, sibling, "artifact:scripts/b.mjs").is_none());
}

/// Regression, and the reason the node-level collapse alone was not enough:
/// `owner_id` is not an endpoint, so it survives the node it names being
/// deleted. An edge owned by a vanished absolute id makes `replace_structure`
/// refuse every later ingest of that file — "structural edge is owned by
/// <absolute id>, not <relative id>" — and with the absolute node already gone
/// there is nothing left for a node-level repair to find. The file becomes
/// permanently un-re-extractable, silently. Reproduced from the live graph:
/// five files stayed blocked after their nodes had already been collapsed.
#[test]
fn ownership_left_behind_by_an_already_deleted_absolute_id_is_reclaimed() {
    let s = store();
    add_node(
        &s,
        "artifact:scripts/a.mjs",
        NodeType::Artifact,
        "scripts/a.mjs",
        NOW,
    );
    add_node(
        &s,
        "artifact:scripts/b.mjs",
        NodeType::Artifact,
        "scripts/b.mjs",
        NOW,
    );
    reinforce(&s, "artifact:scripts/a.mjs", "artifact:scripts/b.mjs", 1);
    // The absolute node is already gone; only its ownership remains.
    let orphaned = "artifact:C:/Users/dev/Repos/MindLeak-export/scripts/a.mjs";
    s.conn
        .execute(
            "UPDATE edges SET owner_id = ?1 WHERE source_id = ?2",
            rusqlite::params![orphaned, "artifact:scripts/a.mjs"],
        )
        .unwrap();
    assert!(s.get_node(orphaned).unwrap().is_none());

    s.repair_workspace_paths(ROOT).unwrap();

    let owner: Option<String> = s
        .conn
        .query_row(
            "SELECT owner_id FROM edges WHERE source_id = ?1",
            rusqlite::params!["artifact:scripts/a.mjs"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        owner.as_deref(),
        Some("artifact:scripts/a.mjs"),
        "ownership must follow the surviving identity"
    );
}
