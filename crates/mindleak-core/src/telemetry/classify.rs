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
