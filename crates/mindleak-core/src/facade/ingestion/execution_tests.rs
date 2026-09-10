use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use rusqlite::functions::FunctionFlags;

use crate::{
    ingest::{self, execution::ExecutionRecord},
    MindLeak, RelationType,
};

fn execution() -> ExecutionRecord {
    ExecutionRecord {
        command: "cargo test".into(),
        exit_code: 0,
        output: "all tests passed".into(),
        cwd: None,
        changed_files: vec![],
        timestamp: 123,
    }
}

// Execution facts committed before attribution, exposing an orphan to prune
// and leaving partial evidence on failure. Both writes must commit together.
#[test]
fn failed_execution_attribution_rolls_back_the_entire_ingestion() {
    for changed_files in [vec![], vec!["src/changed.rs".to_string()]] {
        let engine = MindLeak::open_in_memory().unwrap();
        engine
            .store()
            .conn
            .execute_batch(
                "CREATE TRIGGER refuse_observation BEFORE INSERT ON edges
             WHEN NEW.relation = 'observed'
             BEGIN SELECT RAISE(ABORT, 'attribution unavailable'); END;",
            )
            .unwrap();
        let record = ExecutionRecord {
            changed_files,
            ..execution()
        };
        let error = engine
            .ingest_execution_for_agent("test-agent", &record)
            .unwrap_err();
        assert!(error.to_string().contains("attribution unavailable"));
        let execution = format!("execution:{}", ingest::short_hash("cargo test|123"));
        assert!(
            engine.store().get_node(&execution).unwrap().is_none(),
            "a failed attributed ingest left a committed orphan execution"
        );
        assert!(engine
            .store()
            .get_node("artifact:src/changed.rs")
            .unwrap()
            .is_none());
        assert!(engine
            .store()
            .get_node("agent:test-agent")
            .unwrap()
            .is_none());
    }
}

// Inspect through a separate WAL connection at the exact observation insertion
// boundary. A committed event without attribution is available for prune to reap.
#[test]
fn execution_is_not_visible_to_other_connections_before_attribution() {
    let _serial = crate::db::serialize_db_test();
    let path = std::env::temp_dir().join(format!(
        "mindleak-execution-attribution-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    let engine = MindLeak::open(path.to_str().unwrap()).unwrap();
    let reader = Mutex::new(crate::db::open(path.to_str().unwrap()).unwrap());
    let observed_partial = Arc::new(AtomicBool::new(false));
    let checked = Arc::new(AtomicBool::new(false));
    let partial_flag = observed_partial.clone();
    let checked_flag = checked.clone();
    engine.store().conn.create_scalar_function(
        "inspect_execution_visibility", 1, FunctionFlags::SQLITE_UTF8,
        move |context| {
            let id: String = context.get(0)?;
            let partial: bool = reader.lock().unwrap().query_row(
                "SELECT EXISTS(SELECT 1 FROM nodes WHERE id = ?1)
                 AND NOT EXISTS(SELECT 1 FROM edges WHERE target_id = ?1 AND relation = 'observed')",
                [id], |row| row.get(0),
            )?;
            partial_flag.store(partial, Ordering::SeqCst);
            checked_flag.store(true, Ordering::SeqCst);
            Ok(partial)
        },
    ).unwrap();
    engine
        .store()
        .conn
        .execute_batch(
            "CREATE TEMP TRIGGER inspect_observation BEFORE INSERT ON edges
         WHEN NEW.relation = 'observed' AND NEW.target_id LIKE 'execution:%'
         BEGIN SELECT inspect_execution_visibility(NEW.target_id); END;",
        )
        .unwrap();
    let outcome = engine
        .ingest_execution_for_agent("test-agent", &execution())
        .unwrap();
    let pruning = engine.store().prune_with_signal(crate::now_unix()).unwrap();
    let retained = engine
        .store()
        .get_node(&outcome.node_ids[0])
        .unwrap()
        .is_some();
    drop(engine);
    std::fs::remove_file(path).unwrap();

    assert!(
        checked.load(Ordering::SeqCst),
        "the reader must inspect the write boundary"
    );
    assert!(
        !observed_partial.load(Ordering::SeqCst),
        "maintenance could see a committed execution before its observation"
    );
    assert_eq!(pruning.nodes_removed, 0);
    assert!(retained);
}

#[test]
fn atomic_execution_keeps_counts_observer_identity_and_decay() {
    let engine = MindLeak::open_in_memory().unwrap();
    let now = 1_000_000;
    let record = ExecutionRecord {
        timestamp: now,
        changed_files: vec!["src/changed.rs".into()],
        ..execution()
    };
    let first = ingest::execution::ingest_execution(
        engine.store(),
        &record,
        now,
        &[],
        Some(" agent:test-agent "),
    )
    .unwrap();
    let repeated =
        ingest::execution::ingest_execution(engine.store(), &record, now, &[], Some("test-agent"))
            .unwrap();
    assert_eq!((first.nodes_created, first.edges_created), (2, 1));
    assert_eq!((repeated.nodes_created, repeated.edges_created), (0, 0));
    assert_eq!(first.node_ids, repeated.node_ids);
    assert!(engine
        .store()
        .get_node("agent:agent:test-agent")
        .unwrap()
        .is_none());
    let observation: (f64, i64, i64) = engine
        .store()
        .conn
        .query_row(
            "SELECT half_life_hours, updated_at, reinforcement_count FROM edges
         WHERE source_id = 'agent:test-agent' AND target_id = ?1 AND relation = 'observed'",
            [&first.node_ids[0]],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        observation,
        (RelationType::Modified.default_half_life_hours(), now, 2)
    );
    engine
        .store()
        .prune_with_signal(now + 100 * 86_400)
        .unwrap();
    assert!(engine
        .store()
        .get_node(&first.node_ids[0])
        .unwrap()
        .is_none());
}

#[test]
fn unattributed_execution_still_has_no_invented_observer_or_retention() {
    let engine = MindLeak::open_in_memory().unwrap();
    let outcome = engine.ingest_execution(&execution()).unwrap();
    assert_eq!((outcome.nodes_created, outcome.edges_created), (1, 0));
    let pruning = engine.store().prune_with_signal(crate::now_unix()).unwrap();
    assert_eq!(pruning.nodes_removed, 1);
    assert!(engine
        .store()
        .get_node(&outcome.node_ids[0])
        .unwrap()
        .is_none());
}
