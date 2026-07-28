//! Walking the graph: weighted edges, bounded neighbourhoods and impact radius.
//!
//! Split out of `query.rs` (see `super`); the code is unchanged.

use super::*;

impl GraphStore {
    pub(super) fn edges_for(
        &self,
        id: &str,
        direction: Direction,
        min_weight: f64,
        now: i64,
    ) -> Result<Vec<WeightedEdge>> {
        let mut out = Vec::new();
        if matches!(direction, Direction::Outgoing | Direction::Both) {
            out.extend(self.directed_edges(id, true, min_weight, now)?);
        }
        if matches!(direction, Direction::Incoming | Direction::Both) {
            out.extend(self.directed_edges(id, false, min_weight, now)?);
        }
        Ok(out)
    }

    pub(super) fn directed_edges(
        &self,
        id: &str,
        outgoing: bool,
        min_weight: f64,
        now: i64,
    ) -> Result<Vec<WeightedEdge>> {
        let col = if outgoing { "source_id" } else { "target_id" };
        let sql = format!(
            "SELECT source_id, target_id, relation, weight, half_life_hours, updated_at,
                    first_seen, reinforcement_count
             FROM edges WHERE {col} = ?1"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![id], row_to_raw_edge)?;
        let mut out = Vec::new();
        for r in rows {
            let edge = self.weighted_edge(&r?, now)?;
            if edge.effective >= min_weight {
                out.push(edge);
            }
        }
        out.sort_by(|left, right| {
            right
                .effective
                .partial_cmp(&left.effective)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(out)
    }

    /// Breadth-first, decay-filtered traversal from one or more seed nodes.
    pub fn traverse(
        &self,
        seeds: &[String],
        direction: Direction,
        max_depth: u32,
        min_weight: f64,
        now: i64,
    ) -> Result<Subgraph> {
        self.traverse_where(seeds, direction, max_depth, min_weight, now, |id, edge| {
            Some(if edge.source_id == id {
                edge.target_id.clone()
            } else {
                edge.source_id.clone()
            })
        })
    }

    /// Impact traversal follows dependencies toward affected callers/importers.
    pub fn impact_radius(&self, seeds: &[String], now: i64) -> Result<Subgraph> {
        self.traverse_where(seeds, Direction::Both, 2, 0.0, now, impact_neighbor)
    }

    /// A bounded, relevance-first neighbourhood around `seeds` for visualization.
    /// Best-first expansion keeps the highest decay-weighted (most active) nodes,
    /// and — crucially — follows only each node's strongest `max_fanout` edges, so
    /// a hub node (e.g. an agent observing thousands of nodes) never explodes the
    /// rendered graph. At most `max_nodes` nodes are returned, with only the edges
    /// among them. The extension renders this instead of the full graph so the
    /// visualizer stays responsive no matter how large the graph grows.
    pub fn bounded_neighborhood(
        &self,
        seeds: &[String],
        max_depth: u32,
        max_nodes: usize,
        max_fanout: usize,
        now: i64,
    ) -> Result<Subgraph> {
        let min_weight = self.decay_policy.prune_threshold();
        let mut best: HashMap<String, (u32, f64)> = HashMap::new();
        let mut edge_seen: HashSet<(String, String, String)> = HashSet::new();
        let mut edges: Vec<WeightedEdge> = Vec::new();
        let mut frontier: BinaryHeap<Frontier> = BinaryHeap::new();

        for s in seeds {
            if best.len() >= max_nodes {
                break;
            }
            if self.node_exists(s)? {
                best.insert(s.clone(), (0, 1.0));
                frontier.push(Frontier {
                    score: 1.0,
                    depth: 0,
                    id: s.clone(),
                });
            }
        }

        while let Some(Frontier { id, depth, score }) = frontier.pop() {
            if depth >= max_depth {
                continue;
            }
            let mut incident = self.edges_for(&id, Direction::Both, min_weight, now)?;
            incident.sort_by(|a, b| {
                b.effective
                    .partial_cmp(&a.effective)
                    .unwrap_or(Ordering::Equal)
            });
            for we in incident.into_iter().take(max_fanout) {
                let neighbor = if we.source_id == id {
                    we.target_id.clone()
                } else {
                    we.source_id.clone()
                };
                // Once the node budget is spent, keep edges between already-kept
                // nodes but admit no new ones.
                if !best.contains_key(&neighbor) && best.len() >= max_nodes {
                    continue;
                }
                let key = (
                    we.source_id.clone(),
                    we.target_id.clone(),
                    we.relation.as_str().to_string(),
                );
                if edge_seen.insert(key) {
                    edges.push(we.clone());
                }
                let next_score = score * we.effective;
                let next_depth = depth + 1;
                let improved = match best.get(&neighbor) {
                    Some((_, s)) => next_score > *s,
                    None => true,
                };
                if improved {
                    best.insert(neighbor.clone(), (next_depth, next_score));
                    frontier.push(Frontier {
                        score: next_score,
                        depth: next_depth,
                        id: neighbor,
                    });
                }
            }
        }

        // Drop any edge whose endpoints did not both survive within the budget.
        edges.retain(|we| best.contains_key(&we.source_id) && best.contains_key(&we.target_id));

        let mut nodes = Vec::new();
        for (id, (depth, score)) in &best {
            if let Some(node) = self.get_node(id)? {
                nodes.push(ScoredNode {
                    node,
                    depth: *depth,
                    score: *score,
                });
            }
        }
        nodes.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));

        Ok(Subgraph {
            seed: seeds.to_vec(),
            nodes,
            edges,
        })
    }

    pub(super) fn traverse_where(
        &self,
        seeds: &[String],
        direction: Direction,
        max_depth: u32,
        min_weight: f64,
        now: i64,
        neighbor_for: impl Fn(&str, &WeightedEdge) -> Option<String>,
    ) -> Result<Subgraph> {
        let min_weight = min_weight.max(self.decay_policy.prune_threshold());
        let mut best: HashMap<String, (u32, f64)> = HashMap::new();
        let mut edge_seen: HashSet<(String, String, String)> = HashSet::new();
        let mut edges: Vec<WeightedEdge> = Vec::new();
        let mut queue: VecDeque<(String, u32, f64)> = VecDeque::new();

        for s in seeds {
            if self.node_exists(s)? {
                best.insert(s.clone(), (0, 1.0));
                queue.push_back((s.clone(), 0, 1.0));
            }
        }

        while let Some((id, depth, score)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            for we in self.edges_for(&id, direction, min_weight, now)? {
                let Some(neighbor) = neighbor_for(&id, &we) else {
                    continue;
                };
                let key = (
                    we.source_id.clone(),
                    we.target_id.clone(),
                    we.relation.as_str().to_string(),
                );
                if edge_seen.insert(key) {
                    edges.push(we.clone());
                }
                let next_score = score * we.effective;
                let next_depth = depth + 1;
                let improved = match best.get(&neighbor) {
                    Some((_, s)) => next_score > *s,
                    None => true,
                };
                if improved {
                    best.insert(neighbor.clone(), (next_depth, next_score));
                    queue.push_back((neighbor, next_depth, next_score));
                }
            }
        }

        let mut nodes = Vec::new();
        for (id, (depth, score)) in &best {
            if let Some(node) = self.get_node(id)? {
                nodes.push(ScoredNode {
                    node,
                    depth: *depth,
                    score: *score,
                });
            }
        }
        nodes.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(Subgraph {
            seed: seeds.to_vec(),
            nodes,
            edges,
        })
    }
}
