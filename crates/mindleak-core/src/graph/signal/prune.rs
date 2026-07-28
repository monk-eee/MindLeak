//! Reaping faded signal from the graph.
//!
//! Split out of `signal.rs` (see `super`); the code is unchanged.

use super::*;

impl GraphStore {
    /// Purge decayed noise after surfacing near-expiry proven signal.
    pub fn prune_with_signal(&self, now: i64) -> Result<PruneOutcome> {
        let signal_candidates = self.expiring_signal_candidates(now)?;
        let protected: HashSet<(String, String, String)> = signal_candidates
            .iter()
            .map(|candidate| {
                (
                    candidate.source_id.clone(),
                    candidate.target_id.clone(),
                    candidate.relation.as_str().to_string(),
                )
            })
            .collect();
        let mut stale = Vec::new();
        for raw in self.raw_edges()? {
            let key = (
                raw.source_id.clone(),
                raw.target_id.clone(),
                raw.relation.as_str().to_string(),
            );
            if self.weighted_edge(&raw, now)?.effective < self.decay_policy.prune_threshold()
                && !protected.contains(&key)
            {
                stale.push((raw.source_id, raw.target_id, raw.relation));
            }
        }
        let transaction = self.write_txn()?;
        let mut edges_removed = 0;
        for (source, target, relation) in stale {
            edges_removed += transaction.execute(
                "DELETE FROM edges
                 WHERE source_id = ?1 AND target_id = ?2 AND relation = ?3",
                params![source, target, relation.as_str()],
            )?;
        }
        let executions_removed = transaction.execute(
            "DELETE FROM nodes
             WHERE type = 'execution'
               AND id NOT IN (SELECT source_id FROM edges)
               AND id NOT IN (SELECT target_id FROM edges)",
            [],
        )?;
        let symbols_removed = transaction.execute(
            "DELETE FROM nodes
             WHERE type IN ('symbol', 'package')
               AND NOT EXISTS (
                   SELECT 1 FROM edges
                   WHERE source_id = nodes.id OR target_id = nodes.id
               )",
            [],
        )?;
        let stubs_removed = delete_orphan_artifact_stubs(&transaction)?;
        transaction.commit()?;
        Ok(PruneOutcome {
            edges_removed,
            nodes_removed: executions_removed + symbols_removed + stubs_removed,
            signal_candidates,
        })
    }
}
