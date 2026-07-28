//! Which signals have earned promotion, and committing that consolidation.
//!
//! Split out of `signal.rs` (see `super`); the code is unchanged.

use super::*;

impl GraphStore {
    /// Return high-signal episodic edges within one threshold band of expiry.
    pub fn expiring_signal_candidates(&self, now: i64) -> Result<Vec<SignalCandidate>> {
        let mut statement = self.conn.prepare(
            "SELECT source_id, target_id, relation, weight, half_life_hours, updated_at,
                    first_seen, reinforcement_count FROM edges",
        )?;
        let rows = statement.query_map([], row_to_raw_edge)?;
        let mut candidates = Vec::new();
        for row in rows {
            let raw = row?;
            if !matches!(
                raw.relation,
                RelationType::FailedOn | RelationType::Refactored
            ) {
                continue;
            }
            let edge = self.weighted_edge(&raw, now)?;
            if edge.signal_multiplier > 1.0
                && edge.effective < self.decay_policy.prune_threshold() * 2.0
            {
                candidates.push(SignalCandidate {
                    source_id: edge.source_id,
                    target_id: edge.target_id,
                    relation: edge.relation,
                    effective: edge.effective,
                    signal_multiplier: edge.signal_multiplier,
                    evidence: edge.signal_evidence,
                    updated_at: edge.updated_at,
                });
            }
        }
        candidates.sort_by(|left, right| {
            left.effective
                .partial_cmp(&right.effective)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(candidates)
    }

    /// Aggregate expiring proven signal into subject-level promotion candidates
    /// (ADR-0022): group the high-signal near-expiry edges by (relation, target)
    /// and collect the distinct corroborating source node ids plus the target,
    /// with the provenance window as the span. The emitted ids are opaque MindLeak
    /// strings for the loose plane seam (ADR-0004); this deliberately does *not*
    /// re-implement the count + span gate — the Intent plane's `promote_signals`
    /// reuses it, so a subject with too few distinct corroborators or too narrow a
    /// span is rejected there, not forked here.
    pub fn promotion_candidates(&self, now: i64) -> Result<Vec<PromotionCandidate>> {
        let mut groups: BTreeMap<(String, String), (BTreeSet<String>, i64, i64)> = BTreeMap::new();
        for candidate in self.expiring_signal_candidates(now)? {
            let entry = groups
                .entry((candidate.relation.as_str().to_string(), candidate.target_id))
                .or_insert_with(|| (BTreeSet::new(), candidate.updated_at, candidate.updated_at));
            entry.0.insert(candidate.source_id);
            entry.1 = entry.1.min(candidate.updated_at);
            entry.2 = entry.2.max(candidate.updated_at);
        }
        let candidates = groups
            .into_iter()
            .map(|((_relation, target), (sources, first_seen, last_seen))| {
                let mut evidence_node_ids: Vec<String> = sources.into_iter().collect();
                evidence_node_ids.push(target.clone());
                PromotionCandidate {
                    subject: target,
                    evidence_node_ids,
                    first_seen,
                    last_seen,
                }
            })
            .collect();
        Ok(candidates)
    }

    /// Atomically persist distilled facts and acknowledge their raw signal.
    pub(crate) fn commit_signal_consolidation(
        &self,
        lease_owner: &str,
        now: i64,
        candidates: &[SignalCandidate],
        nodes: &[Node],
        edges: &[Edge],
    ) -> Result<(WriteOutcome, usize, usize)> {
        let transaction = self.write_txn()?;
        let lease_valid: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM maintenance_leases
                 WHERE name = 'signal_consolidation' AND owner = ?1
                   AND lease_expires_at > ?2
             )",
            params![lease_owner, now],
            |row| row.get(0),
        )?;
        if !lease_valid {
            return Err(MindLeakError::Busy(
                "signal consolidation lease expired".to_string(),
            ));
        }
        let edges_removed = delete_signal_edges_on(&transaction, candidates)?;
        if edges_removed != candidates.len() {
            return Err(MindLeakError::Other(
                "signal candidates changed during consolidation".to_string(),
            ));
        }
        let mut outcome = WriteOutcome::default();
        for node in nodes {
            if upsert_node_on(&transaction, node)? {
                outcome.nodes_created += 1;
            }
        }
        for edge in edges {
            if upsert_edge_on(&transaction, edge, None)? {
                outcome.edges_created += 1;
            }
        }
        let nodes_removed = delete_orphan_signal_nodes_on(&transaction, candidates)?;
        transaction.commit()?;
        Ok((outcome, edges_removed, nodes_removed))
    }
}
