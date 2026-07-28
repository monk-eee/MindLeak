//! Per-agent views: the roster, one agent's attention, and footprint overlap.
//!
//! Split out of `query.rs` (see `super`); the code is unchanged.

use super::*;

impl GraphStore {
    /// The agent roster: each `agent` node with its active observation count and
    /// last-active time (most recently active first).
    pub fn list_agents(&self, now: i64) -> Result<Vec<AgentActivity>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, label, last_accessed_at FROM nodes
             WHERE type = 'agent' ORDER BY last_accessed_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, label, last_active) = row?;
            let observations = self
                .directed_edges(&id, true, self.decay_policy.prune_threshold(), now)?
                .into_iter()
                .filter(|edge| edge.relation == RelationType::Observed)
                .count() as i64;
            out.push(AgentActivity {
                id,
                label,
                last_active,
                observations,
            });
        }
        Ok(out)
    }

    /// Highest active attention edges for one agent, strictly bounded by limit.
    pub fn working_set(&self, agent: &str, limit: usize, now: i64) -> Result<Vec<WorkingSetItem>> {
        let agent_id = if agent.starts_with("agent:") {
            agent.to_string()
        } else {
            format!("agent:{agent}")
        };
        let mut statement = self.conn.prepare(
            "SELECT source_id, target_id, relation, weight, half_life_hours, updated_at,
                    first_seen, reinforcement_count
             FROM edges
               WHERE source_id = ?1 AND relation = 'observed' AND updated_at <= ?2",
        )?;
        let rows = statement.query_map(params![agent_id, now], row_to_raw_edge)?;
        let mut items = Vec::new();
        for row in rows {
            let raw = row?;
            let edge = self.weighted_edge(&raw, now)?;
            if edge.effective < self.decay_policy.prune_threshold() {
                continue;
            }
            if let Some(node) = self.get_node(&edge.target_id)? {
                items.push(WorkingSetItem {
                    node,
                    attention: edge.effective,
                    observation_count: raw.reinforcement_count,
                    observation_span_hours: ((raw.updated_at - raw.first_seen) as f64 / 3600.0)
                        .max(0.0),
                    last_observed_at: raw.updated_at,
                });
            }
        }
        items.sort_by(|left, right| {
            right
                .attention
                .partial_cmp(&left.attention)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| right.last_observed_at.cmp(&left.last_observed_at))
                .then_with(|| left.node.id.cmp(&right.node.id))
        });
        items.truncate(limit.clamp(1, self.working_set_size));
        Ok(items)
    }

    /// Other agents' decay-active footprint on requested artifact/symbol ids
    /// (ADR-0024). Follows direct observation or one observed execution/intent
    /// hop into mutation evidence. Read-only and advisory.
    pub fn agent_footprint_overlap(
        &self,
        node_ids: &[String],
        exclude_agent: Option<&str>,
        now: i64,
    ) -> Result<Vec<AgentFootprintOverlap>> {
        let targets = node_ids.iter().cloned().collect::<HashSet<_>>();
        if targets.is_empty() {
            return Ok(Vec::new());
        }
        let excluded = exclude_agent.map(|agent| {
            if agent.starts_with("agent:") {
                agent.to_string()
            } else {
                format!("agent:{agent}")
            }
        });
        let threshold = self.decay_policy.prune_threshold();
        let mut agents = self
            .conn
            .prepare("SELECT id FROM nodes WHERE type = 'agent'")?;
        let rows = agents.query_map([], |row| row.get::<_, String>(0))?;
        let mut best: HashMap<(String, String), AgentFootprintOverlap> = HashMap::new();
        for row in rows {
            let agent_id = row?;
            if excluded.as_deref() == Some(agent_id.as_str()) {
                continue;
            }
            for observed in self.directed_edges(&agent_id, true, threshold, now)? {
                if observed.relation != RelationType::Observed {
                    continue;
                }
                if targets.contains(&observed.target_id) {
                    retain_best_overlap(
                        &mut best,
                        AgentFootprintOverlap {
                            agent_id: agent_id.clone(),
                            node_id: observed.target_id.clone(),
                            via_node_id: observed.target_id.clone(),
                            relation: RelationType::Observed,
                            effective: observed.effective,
                            last_observed_at: observed.updated_at,
                        },
                    );
                }
                for mutation in self.directed_edges(&observed.target_id, true, threshold, now)? {
                    if !matches!(
                        mutation.relation,
                        RelationType::Modified | RelationType::Refactored | RelationType::Fixed
                    ) || !targets.contains(&mutation.target_id)
                    {
                        continue;
                    }
                    let effective = observed.effective * mutation.effective;
                    if effective < threshold {
                        continue;
                    }
                    retain_best_overlap(
                        &mut best,
                        AgentFootprintOverlap {
                            agent_id: agent_id.clone(),
                            node_id: mutation.target_id.clone(),
                            via_node_id: observed.target_id.clone(),
                            relation: mutation.relation,
                            effective,
                            last_observed_at: observed.updated_at,
                        },
                    );
                }
            }
        }
        let mut overlaps = best.into_values().collect::<Vec<_>>();
        overlaps.sort_by(|left, right| {
            right
                .effective
                .partial_cmp(&left.effective)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.agent_id.cmp(&right.agent_id))
                .then_with(|| left.node_id.cmp(&right.node_id))
        });
        Ok(overlaps)
    }
}
