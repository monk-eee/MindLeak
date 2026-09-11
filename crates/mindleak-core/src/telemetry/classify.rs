//! Classify a recorded event by its tool name.

pub(super) fn is_memory_read(name: &str) -> bool {
    matches!(
        name,
        "working_set" | "recall" | "get_impact_radius" | "graph_multi_hop_query" | "check_overlap"
    )
}

pub(super) fn is_attributed_write(name: &str) -> bool {
    matches!(
        name,
        "ingest_file"
            | "ingest_execution"
            | "ingest_commit"
            | "repair_commit_attribution"
            | "record_architectural_decision"
            | "boost_entity"
    )
}

pub(super) fn is_background_read(name: &str) -> bool {
    matches!(
        name,
        "graph_stats" | "graph_snapshot" | "telemetry_snapshot" | "list_agents"
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn commit_repair_counts_as_an_attributed_write_not_a_memory_read() {
        assert!(super::is_attributed_write("repair_commit_attribution"));
        assert!(!super::is_memory_read("repair_commit_attribution"));
        assert!(!super::is_background_read("repair_commit_attribution"));
    }
}
