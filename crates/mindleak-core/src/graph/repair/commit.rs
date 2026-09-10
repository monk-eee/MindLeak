use std::collections::BTreeSet;

use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::json;

use crate::graph::signal::row_to_raw_edge;
use crate::graph::types::RawEdge;
use crate::graph::writes::{node_exists_on, upsert_edge_on, upsert_node_on};
use crate::graph::GraphStore;
use crate::ingest::git::{is_full_commit_sha, CommitRecord};
use crate::{telemetry, Edge, MindLeakError, Node, NodeType, RelationType, Result};

const MAX_REPAIR_EDGES: usize = 10_000;

#[derive(Debug, Default, Serialize)]
pub struct CommitRepairOutcome {
    pub intent_id: String,
    pub removed_edges: usize,
    pub added_edges: usize,
    pub corrected_timestamps: usize,
    pub metadata_changed: bool,
    pub audit_id: Option<i64>,
}

#[derive(Serialize)]
struct StoredRefactor {
    #[serde(flatten)]
    edge: RawEdge,
    owner_id: Option<String>,
}

impl GraphStore {
    pub(crate) fn repair_commit_attribution(
        &self,
        agent: &str,
        verified: &CommitRecord,
        reason: &str,
        now: i64,
        roots: &[&str],
    ) -> Result<CommitRepairOutcome> {
        let sha = verified
            .sha
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !is_full_commit_sha(&sha) || agent.trim().is_empty() || reason.trim().is_empty() {
            return Err(MindLeakError::InvalidArgument(
                "commit repair requires a full hash, agent and reason".into(),
            ));
        }
        let intent_id = format!("intent:{sha}");
        let desired: BTreeSet<_> = verified
            .changed_paths(roots)
            .into_iter()
            .map(|path| format!("artifact:{path}"))
            .collect();
        if desired.len() > MAX_REPAIR_EDGES {
            return Err(MindLeakError::InvalidArgument(
                "commit repair exceeds 10000 edges".into(),
            ));
        }
        let transaction = self.write_txn()?;
        let before_node = self.get_node(&intent_id)?.ok_or_else(|| {
            MindLeakError::InvalidArgument("commit repair requires an existing intent node".into())
        })?;
        if before_node.node_type != NodeType::Intent {
            return Err(MindLeakError::InvalidArgument(
                "the stored commit is not an intent node".into(),
            ));
        }
        let expected_node = verified.intent_node(&intent_id);
        let before_edges = stored_refactors(&transaction, &intent_id)?;
        let previous: BTreeSet<_> = before_edges
            .iter()
            .map(|record| record.edge.target_id.clone())
            .collect();
        let mut outcome = CommitRepairOutcome {
            intent_id: intent_id.clone(),
            removed_edges: previous.difference(&desired).count(),
            added_edges: desired.difference(&previous).count(),
            corrected_timestamps: before_edges
                .iter()
                .filter(|record| {
                    desired.contains(&record.edge.target_id)
                        && (record.edge.updated_at != verified.timestamp
                            || record.edge.first_seen != verified.timestamp)
                })
                .count(),
            metadata_changed: before_node.label != expected_node.label
                || before_node.content != expected_node.content
                || before_node.created_at != expected_node.created_at,
            audit_id: None,
        };
        if outcome.removed_edges == 0
            && outcome.added_edges == 0
            && outcome.corrected_timestamps == 0
            && !outcome.metadata_changed
        {
            transaction.commit()?;
            return Ok(outcome);
        }
        for target in previous.difference(&desired) {
            transaction.execute(
                "DELETE FROM edges WHERE source_id = ?1 AND target_id = ?2 AND relation = 'refactored'",
                params![intent_id, target],
            )?;
        }
        for target in &desired {
            if previous.contains(target) {
                transaction.execute(
                    "UPDATE edges SET updated_at = ?3, first_seen = ?3
                     WHERE source_id = ?1 AND target_id = ?2 AND relation = 'refactored'
                       AND (updated_at <> ?3 OR first_seen <> ?3)",
                    params![intent_id, target, verified.timestamp],
                )?;
            } else {
                if !node_exists_on(&transaction, target)? {
                    upsert_node_on(
                        &transaction,
                        &Node::new(
                            target,
                            NodeType::Artifact,
                            &target["artifact:".len()..],
                            now,
                        ),
                    )?;
                }
                upsert_edge_on(
                    &transaction,
                    &Edge::new(
                        &intent_id,
                        target,
                        RelationType::Refactored,
                        verified.timestamp,
                    ),
                    None,
                )?;
            }
        }
        if outcome.metadata_changed {
            transaction.execute(
                "UPDATE nodes SET label = ?2, content = ?3, created_at = ?4 WHERE id = ?1",
                params![
                    intent_id,
                    expected_node.label,
                    expected_node.content,
                    verified.timestamp
                ],
            )?;
        }
        let detail = json!({
            "schema_version": 1,
            "agent_id": agent,
            "reason": reason,
            "commit_id": intent_id,
            "verified": {
                "sha": sha,
                "message": verified.message,
                "timestamp": verified.timestamp,
                "changed_files": verified.changed_files,
            },
            "before": { "node": before_node, "edges": before_edges },
            "after": {
                "node": self.get_node(&intent_id)?,
                "edges": stored_refactors(&transaction, &intent_id)?,
            },
        });
        if detail.to_string().len() > 4_194_304 {
            return Err(MindLeakError::InvalidArgument(
                "commit repair audit exceeds 4 MiB; no changes committed".into(),
            ));
        }
        telemetry::record(
            &transaction,
            now,
            "evidence_repair",
            "repair_commit_attribution",
            "ok",
            None,
            Some(&detail),
        )?;
        outcome.audit_id = Some(transaction.last_insert_rowid());
        transaction.commit()?;
        Ok(outcome)
    }
}

fn stored_refactors(connection: &Connection, intent_id: &str) -> Result<Vec<StoredRefactor>> {
    let mut statement = connection.prepare(
        "SELECT source_id, target_id, relation, weight, half_life_hours,
                updated_at, first_seen, reinforcement_count, owner_id
         FROM edges WHERE source_id = ?1 AND relation = 'refactored'
         ORDER BY target_id LIMIT 10001",
    )?;
    let rows = statement.query_map(params![intent_id], |row| {
        Ok(StoredRefactor {
            edge: row_to_raw_edge(row)?,
            owner_id: row.get(8)?,
        })
    })?;
    let records: Vec<_> = rows.collect::<rusqlite::Result<_>>()?;
    if records.len() > MAX_REPAIR_EDGES {
        return Err(MindLeakError::InvalidArgument(
            "stored commit attribution exceeds 10000 edges".into(),
        ));
    }
    Ok(records)
}
