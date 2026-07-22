//! Graph storage and decay-aware traversal over SQLite.

use std::collections::{HashMap, HashSet, VecDeque};

use rusqlite::{params, Connection, Row};
use serde::Serialize;

use crate::decay::PRUNE_THRESHOLD;
use crate::error::Result;
use crate::model::{Edge, Node, NodeType, RelationType};

/// Direction of edge expansion during traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Outgoing,
    Incoming,
    Both,
}

/// An edge annotated with its current time-decayed effective weight.
#[derive(Debug, Clone, Serialize)]
pub struct WeightedEdge {
    pub source_id: String,
    pub target_id: String,
    pub relation: RelationType,
    pub base_weight: f64,
    pub effective: f64,
    pub half_life_hours: f64,
    pub updated_at: i64,
}

/// A node reached during traversal, with its depth and path score.
#[derive(Debug, Clone, Serialize)]
pub struct ScoredNode {
    #[serde(flatten)]
    pub node: Node,
    pub depth: u32,
    pub score: f64,
}

/// The result of a graph query: reachable nodes plus the traversed edges.
#[derive(Debug, Clone, Serialize)]
pub struct Subgraph {
    pub seed: Vec<String>,
    pub nodes: Vec<ScoredNode>,
    pub edges: Vec<WeightedEdge>,
}

/// Outcome of a mutation (created counts + touched ids).
#[derive(Debug, Clone, Default, Serialize)]
pub struct WriteOutcome {
    pub nodes_created: usize,
    pub edges_created: usize,
    pub node_ids: Vec<String>,
}

/// An `agent` node with its current activity (roster entry).
#[derive(Debug, Clone, Serialize)]
pub struct AgentActivity {
    pub id: String,
    pub label: String,
    pub observations: i64,
    pub last_active: i64,
}

/// The persistent graph store.
pub struct GraphStore {
    conn: Connection,
}

impl GraphStore {
    pub fn new(conn: Connection) -> Self {
        GraphStore { conn }
    }

    // ---- writes -------------------------------------------------------------

    /// Insert or reinforce a node. Returns true if newly created.
    pub fn upsert_node(&self, node: &Node) -> Result<bool> {
        let existed = self.node_exists(&node.id)?;
        self.conn.execute(
            "INSERT INTO nodes (id, type, label, content, created_at, last_accessed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                 label = excluded.label,
                 content = COALESCE(excluded.content, nodes.content),
                 last_accessed_at = excluded.last_accessed_at",
            params![
                node.id,
                node.node_type.as_str(),
                node.label,
                node.content,
                node.created_at,
                node.last_accessed_at,
            ],
        )?;
        Ok(!existed)
    }

    /// Insert or reinforce an edge. Returns true if newly created.
    /// Re-ingesting an existing edge nudges its weight up and resets its decay clock.
    pub fn upsert_edge(&self, edge: &Edge) -> Result<bool> {
        let existed = self.edge_exists(&edge.source_id, &edge.target_id, edge.relation)?;
        self.conn.execute(
            "INSERT INTO edges (source_id, target_id, relation, weight, half_life_hours, updated_at, first_seen, reinforcement_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1)
             ON CONFLICT(source_id, target_id, relation) DO UPDATE SET
                 weight = MIN(1.0, edges.weight + 0.05),
                 half_life_hours = excluded.half_life_hours,
                 updated_at = excluded.updated_at,
                 reinforcement_count = edges.reinforcement_count + 1",
            params![
                edge.source_id,
                edge.target_id,
                edge.relation.as_str(),
                edge.weight,
                edge.half_life_hours,
                edge.updated_at,
            ],
        )?;
        Ok(!existed)
    }

    /// Elevate a node in real time (e.g. the editor focused this file):
    /// refresh its access time and reset the decay clock on incident edges.
    pub fn boost(&self, id: &str, now: i64) -> Result<bool> {
        if !self.node_exists(id)? {
            return Ok(false);
        }
        self.conn.execute(
            "UPDATE nodes SET last_accessed_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        self.conn.execute(
            "UPDATE edges SET weight = MIN(1.0, weight + 0.05), updated_at = ?2
             WHERE source_id = ?1 OR target_id = ?1",
            params![id, now],
        )?;
        Ok(true)
    }

    // ---- reads --------------------------------------------------------------

    pub fn node_exists(&self, id: &str) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(1) FROM nodes WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    fn edge_exists(&self, source: &str, target: &str, relation: RelationType) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(1) FROM edges WHERE source_id = ?1 AND target_id = ?2 AND relation = ?3",
            params![source, target, relation.as_str()],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn get_node(&self, id: &str) -> Result<Option<Node>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, type, label, content, created_at, last_accessed_at FROM nodes WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_node(row)?)),
            None => Ok(None),
        }
    }

    /// Full-text search over node labels + content. Returns best matches first.
    pub fn search_nodes(&self, query: &str, limit: usize) -> Result<Vec<Node>> {
        let match_query = build_fts_query(query);
        if match_query.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT n.id, n.type, n.label, n.content, n.created_at, n.last_accessed_at
             FROM nodes_fts f
             JOIN nodes n ON n.id = f.id
             WHERE nodes_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![match_query, limit as i64], row_to_node)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Resolve a seed argument to one or more node ids.
    /// An exact node id wins; otherwise fall back to full-text search.
    pub fn resolve_seed(&self, seed: &str, limit: usize) -> Result<Vec<String>> {
        if self.node_exists(seed)? {
            return Ok(vec![seed.to_string()]);
        }
        // Try an artifact-path convenience form (`src/x.ts` -> `artifact:src/x.ts`).
        let artifact = format!("artifact:{seed}");
        if self.node_exists(&artifact)? {
            return Ok(vec![artifact]);
        }
        let hits = self.search_nodes(seed, limit)?;
        Ok(hits.into_iter().map(|n| n.id).collect())
    }

    fn edges_for(
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

    fn directed_edges(
        &self,
        id: &str,
        outgoing: bool,
        min_weight: f64,
        now: i64,
    ) -> Result<Vec<WeightedEdge>> {
        let col = if outgoing { "source_id" } else { "target_id" };
        let sql = format!(
            "SELECT source_id, target_id, relation, weight, half_life_hours, updated_at,
                    effective_weight(weight, signal_half_life(half_life_hours, reinforcement_count, first_seen, updated_at), updated_at, ?2) AS eff
             FROM edges
             WHERE {col} = ?1
               AND effective_weight(weight, signal_half_life(half_life_hours, reinforcement_count, first_seen, updated_at), updated_at, ?2) >= ?3
             ORDER BY eff DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![id, now, min_weight], row_to_weighted_edge)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
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
                let neighbor = if we.source_id == id {
                    we.target_id.clone()
                } else {
                    we.source_id.clone()
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

    /// Purge edges whose effective weight has decayed below the threshold, then
    /// drop orphaned execution nodes (raw noise with no surviving links).
    pub fn prune(&self, now: i64) -> Result<(usize, usize)> {
        let edges_removed = self.conn.execute(
            "DELETE FROM edges WHERE effective_weight(weight, signal_half_life(half_life_hours, reinforcement_count, first_seen, updated_at), updated_at, ?1) < ?2",
            params![now, PRUNE_THRESHOLD],
        )?;
        let nodes_removed = self.conn.execute(
            "DELETE FROM nodes
             WHERE type = 'execution'
               AND id NOT IN (SELECT source_id FROM edges)
               AND id NOT IN (SELECT target_id FROM edges)",
            [],
        )?;
        Ok((edges_removed, nodes_removed))
    }

    /// Total node / edge counts (for status displays).
    pub fn counts(&self, now: i64) -> Result<(i64, i64)> {
        let nodes: i64 = self
            .conn
            .query_row("SELECT COUNT(1) FROM nodes", [], |r| r.get(0))?;
        let edges: i64 = self.conn.query_row(
            "SELECT COUNT(1) FROM edges WHERE effective_weight(weight, signal_half_life(half_life_hours, reinforcement_count, first_seen, updated_at), updated_at, ?1) >= ?2",
            params![now, PRUNE_THRESHOLD],
            |r| r.get(0),
        )?;
        Ok((nodes, edges))
    }

    /// The agent roster: each `agent` node with its active observation count and
    /// last-active time (most recently active first).
    pub fn list_agents(&self, now: i64) -> Result<Vec<AgentActivity>> {
        let mut stmt = self.conn.prepare(
            "SELECT n.id, n.label, n.last_accessed_at,
                    (SELECT COUNT(1) FROM edges e
                      WHERE e.source_id = n.id AND e.relation = 'observed'
                        AND effective_weight(e.weight, e.half_life_hours, e.updated_at, ?1) >= ?2)
             FROM nodes n
             WHERE n.type = 'agent'
             ORDER BY n.last_accessed_at DESC",
        )?;
        let rows = stmt.query_map(params![now, PRUNE_THRESHOLD], |r| {
            Ok(AgentActivity {
                id: r.get(0)?,
                label: r.get(1)?,
                last_active: r.get(2)?,
                observations: r.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// A snapshot of the most recently accessed nodes and the active edges
    /// among them — used by the editor visualizer.
    pub fn snapshot(&self, limit: usize, now: i64) -> Result<Subgraph> {
        let mut stmt = self.conn.prepare(
            "SELECT id, type, label, content, created_at, last_accessed_at
             FROM nodes ORDER BY last_accessed_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], row_to_node)?;
        let mut nodes = Vec::new();
        let mut ids: HashSet<String> = HashSet::new();
        for r in rows {
            let node = r?;
            ids.insert(node.id.clone());
            nodes.push(ScoredNode {
                node,
                depth: 0,
                score: 1.0,
            });
        }

        let mut edge_stmt = self.conn.prepare(
            "SELECT source_id, target_id, relation, weight, half_life_hours, updated_at,
                    effective_weight(weight, signal_half_life(half_life_hours, reinforcement_count, first_seen, updated_at), updated_at, ?1) AS eff
             FROM edges
             WHERE effective_weight(weight, signal_half_life(half_life_hours, reinforcement_count, first_seen, updated_at), updated_at, ?1) >= ?2",
        )?;
        let edge_rows = edge_stmt.query_map(params![now, PRUNE_THRESHOLD], row_to_weighted_edge)?;
        let mut edges = Vec::new();
        for r in edge_rows {
            let we = r?;
            if ids.contains(&we.source_id) && ids.contains(&we.target_id) {
                edges.push(we);
            }
        }

        Ok(Subgraph {
            seed: vec![],
            nodes,
            edges,
        })
    }
}

fn row_to_node(row: &Row) -> rusqlite::Result<Node> {
    let type_str: String = row.get(1)?;
    Ok(Node {
        id: row.get(0)?,
        node_type: NodeType::from_tag(&type_str).unwrap_or(NodeType::Artifact),
        label: row.get(2)?,
        content: row.get(3)?,
        created_at: row.get(4)?,
        last_accessed_at: row.get(5)?,
    })
}

fn row_to_weighted_edge(row: &Row) -> rusqlite::Result<WeightedEdge> {
    let relation_str: String = row.get(2)?;
    Ok(WeightedEdge {
        source_id: row.get(0)?,
        target_id: row.get(1)?,
        relation: RelationType::from_tag(&relation_str).unwrap_or(RelationType::RelatesTo),
        base_weight: row.get(3)?,
        half_life_hours: row.get(4)?,
        updated_at: row.get(5)?,
        effective: row.get(6)?,
    })
}

/// Build a safe FTS5 MATCH query from arbitrary user text: split into alnum
/// terms and OR them together. Returns "" when nothing usable remains.
fn build_fts_query(input: &str) -> String {
    let terms: Vec<String> = input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2)
        .map(|t| format!("\"{t}\""))
        .collect();
    terms.join(" OR ")
}

#[cfg(test)]
mod tests {
    // Generated by AI (UnitTest MCP)
    use super::*;
    use crate::db;
    use crate::model::{Edge, Node, NodeType, RelationType};

    const NOW: i64 = 1_000_000;
    const HOUR: i64 = 3600;

    fn store() -> GraphStore {
        GraphStore::new(db::open_in_memory().unwrap())
    }

    fn add_node(s: &GraphStore, id: &str, ty: NodeType, label: &str, now: i64) {
        s.upsert_node(&Node::new(id, ty, label, now)).unwrap();
    }

    fn raw_edge(
        src: &str,
        tgt: &str,
        rel: RelationType,
        weight: f64,
        half_life: f64,
        updated_at: i64,
    ) -> Edge {
        Edge {
            source_id: src.to_string(),
            target_id: tgt.to_string(),
            relation: rel,
            weight,
            half_life_hours: half_life,
            updated_at,
        }
    }

    #[test]
    fn upsert_node_reports_created_then_reinforced() {
        let s = store();
        let node = Node::new("artifact:a", NodeType::Artifact, "a", NOW);
        assert!(s.upsert_node(&node).unwrap()); // created
        assert!(!s.upsert_node(&node).unwrap()); // reinforced, not created
        assert!(s.get_node("artifact:a").unwrap().is_some());
    }

    #[test]
    fn upsert_edge_reinforces_weight_on_conflict() {
        let s = store();
        add_node(&s, "artifact:a", NodeType::Artifact, "a", NOW);
        add_node(&s, "artifact:b", NodeType::Artifact, "b", NOW);
        let e = raw_edge(
            "artifact:a",
            "artifact:b",
            RelationType::Modified,
            0.5,
            48.0,
            NOW,
        );
        assert!(s.upsert_edge(&e).unwrap()); // created
        assert!(!s.upsert_edge(&e).unwrap()); // reinforced

        let sub = s
            .traverse(&["artifact:a".into()], Direction::Outgoing, 1, 0.0, NOW)
            .unwrap();
        let edge = sub
            .edges
            .iter()
            .find(|x| x.target_id == "artifact:b")
            .unwrap();
        assert!((edge.base_weight - 0.55).abs() < 1e-9);
    }

    #[test]
    fn proven_signal_edge_outlives_a_one_off_at_the_same_age() {
        // ADR-0005: an edge reinforced repeatedly across a wide span earns a
        // longer half-life, so at equal age it keeps more weight than a one-off.
        // Same-session spam (narrow span) would NOT earn this.
        let s = store();
        add_node(&s, "artifact:a", NodeType::Artifact, "a", NOW);
        add_node(&s, "artifact:one_off", NodeType::Artifact, "o", NOW);
        add_node(&s, "artifact:proven", NodeType::Artifact, "p", NOW);

        // one-off: a single reinforcement at NOW.
        s.upsert_edge(&raw_edge(
            "artifact:a",
            "artifact:one_off",
            RelationType::Modified,
            1.0,
            24.0,
            NOW,
        ))
        .unwrap();
        // proven: 3 reinforcements spread across 100h, ending at NOW.
        for t in [NOW - 100 * HOUR, NOW - 50 * HOUR, NOW] {
            s.upsert_edge(&raw_edge(
                "artifact:a",
                "artifact:proven",
                RelationType::Modified,
                1.0,
                24.0,
                t,
            ))
            .unwrap();
        }

        // Four days after their (shared) last update.
        let now = NOW + 4 * 24 * HOUR;
        let sub = s
            .traverse(&["artifact:a".into()], Direction::Outgoing, 1, 0.0, now)
            .unwrap();
        let one_off = sub
            .edges
            .iter()
            .find(|e| e.target_id == "artifact:one_off")
            .unwrap();
        let proven = sub
            .edges
            .iter()
            .find(|e| e.target_id == "artifact:proven")
            .unwrap();
        assert!(proven.effective > one_off.effective);
    }

    #[test]
    fn search_nodes_finds_by_label() {
        let s = store();
        add_node(
            &s,
            "symbol:x:validateSession",
            NodeType::Symbol,
            "validate session handler",
            NOW,
        );
        let hits = s.search_nodes("session", 5).unwrap();
        assert!(hits.iter().any(|n| n.id == "symbol:x:validateSession"));
        assert!(s.search_nodes("a", 5).unwrap().is_empty()); // single char -> no query
    }

    #[test]
    fn resolve_seed_exact_convenience_and_fts() {
        let s = store();
        add_node(&s, "artifact:src/x.rs", NodeType::Artifact, "src/x.rs", NOW);
        add_node(
            &s,
            "intent:abc",
            NodeType::Intent,
            "unique_token_zzz decision",
            NOW,
        );

        assert_eq!(
            s.resolve_seed("artifact:src/x.rs", 3).unwrap(),
            vec!["artifact:src/x.rs"]
        );
        assert_eq!(
            s.resolve_seed("src/x.rs", 3).unwrap(),
            vec!["artifact:src/x.rs"]
        );
        assert_eq!(
            s.resolve_seed("unique_token_zzz", 3).unwrap(),
            vec!["intent:abc"]
        );
    }

    #[test]
    fn traverse_respects_depth_and_min_weight() {
        let s = store();
        for id in ["a", "b", "c", "d"] {
            add_node(&s, id, NodeType::Artifact, id, NOW);
        }
        s.upsert_edge(&raw_edge("a", "b", RelationType::Calls, 1.0, 168.0, NOW))
            .unwrap();
        s.upsert_edge(&raw_edge("b", "c", RelationType::Calls, 1.0, 168.0, NOW))
            .unwrap();
        // Decayed edge (10 half-lives old) must be filtered out.
        s.upsert_edge(&raw_edge(
            "a",
            "d",
            RelationType::Calls,
            1.0,
            1.0,
            NOW - 10 * HOUR,
        ))
        .unwrap();

        let depth1 = s
            .traverse(&["a".into()], Direction::Outgoing, 1, 0.2, NOW)
            .unwrap();
        let ids1: Vec<_> = depth1.nodes.iter().map(|n| n.node.id.clone()).collect();
        assert!(ids1.contains(&"b".to_string()));
        assert!(!ids1.contains(&"c".to_string())); // beyond depth 1
        assert!(!ids1.contains(&"d".to_string())); // decayed below min_weight

        let depth2 = s
            .traverse(&["a".into()], Direction::Outgoing, 2, 0.2, NOW)
            .unwrap();
        let ids2: Vec<_> = depth2.nodes.iter().map(|n| n.node.id.clone()).collect();
        assert!(ids2.contains(&"c".to_string()));
    }

    #[test]
    fn traverse_both_directions_reaches_incoming() {
        let s = store();
        add_node(&s, "caller", NodeType::Symbol, "caller", NOW);
        add_node(&s, "target", NodeType::Symbol, "target", NOW);
        s.upsert_edge(&raw_edge(
            "caller",
            "target",
            RelationType::Calls,
            1.0,
            168.0,
            NOW,
        ))
        .unwrap();

        let out = s
            .traverse(&["target".into()], Direction::Outgoing, 1, 0.05, NOW)
            .unwrap();
        assert!(!out.nodes.iter().any(|n| n.node.id == "caller"));

        let both = s
            .traverse(&["target".into()], Direction::Both, 1, 0.05, NOW)
            .unwrap();
        assert!(both.nodes.iter().any(|n| n.node.id == "caller"));
    }

    #[test]
    fn snapshot_only_includes_edges_between_returned_nodes() {
        let s = store();
        add_node(&s, "n1", NodeType::Artifact, "n1", NOW - 2);
        add_node(&s, "n2", NodeType::Artifact, "n2", NOW - 1);
        add_node(&s, "n3", NodeType::Artifact, "n3", NOW);
        s.upsert_edge(&raw_edge("n3", "n2", RelationType::Calls, 1.0, 168.0, NOW))
            .unwrap();
        s.upsert_edge(&raw_edge("n3", "n1", RelationType::Calls, 1.0, 168.0, NOW))
            .unwrap();

        let snap = s.snapshot(2, NOW).unwrap();
        let ids: Vec<_> = snap.nodes.iter().map(|n| n.node.id.clone()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"n3".to_string()) && ids.contains(&"n2".to_string()));
        // Edge to n1 excluded because n1 is outside the snapshot node set.
        assert_eq!(snap.edges.len(), 1);
        assert_eq!(snap.edges[0].target_id, "n2");
    }

    #[test]
    fn prune_removes_decayed_edges_and_orphan_executions() {
        let s = store();
        add_node(&s, "a", NodeType::Artifact, "a", NOW);
        add_node(&s, "c", NodeType::Artifact, "c", NOW);
        add_node(&s, "b", NodeType::Artifact, "b", NOW);
        add_node(
            &s,
            "execution:orphan",
            NodeType::Execution,
            "orphan run",
            NOW,
        );
        s.upsert_edge(&raw_edge("a", "c", RelationType::Calls, 1.0, 168.0, NOW))
            .unwrap();
        s.upsert_edge(&raw_edge(
            "a",
            "b",
            RelationType::Modified,
            1.0,
            1.0,
            NOW - 10 * HOUR,
        ))
        .unwrap();

        let (edges_removed, nodes_removed) = s.prune(NOW).unwrap();
        assert_eq!(edges_removed, 1); // only the decayed a->b
        assert_eq!(nodes_removed, 1); // the orphan execution node
        assert!(s.get_node("execution:orphan").unwrap().is_none());
        assert!(s.get_node("a").unwrap().is_some());
    }

    #[test]
    fn boost_resets_decay_clock() {
        let s = store();
        add_node(&s, "a", NodeType::Artifact, "a", NOW);
        add_node(&s, "b", NodeType::Artifact, "b", NOW);
        s.upsert_edge(&raw_edge(
            "a",
            "b",
            RelationType::Modified,
            1.0,
            1.0,
            NOW - 10 * HOUR,
        ))
        .unwrap();

        let before = s
            .traverse(&["a".into()], Direction::Outgoing, 1, 0.05, NOW)
            .unwrap();
        assert!(!before.nodes.iter().any(|n| n.node.id == "b"));

        assert!(s.boost("a", NOW).unwrap());
        assert!(!s.boost("missing", NOW).unwrap());

        let after = s
            .traverse(&["a".into()], Direction::Outgoing, 1, 0.05, NOW)
            .unwrap();
        assert!(after.nodes.iter().any(|n| n.node.id == "b"));
    }

    #[test]
    fn counts_reflects_active_edges_only() {
        let s = store();
        add_node(&s, "a", NodeType::Artifact, "a", NOW);
        add_node(&s, "b", NodeType::Artifact, "b", NOW);
        s.upsert_edge(&raw_edge("a", "b", RelationType::Calls, 1.0, 168.0, NOW))
            .unwrap();
        s.upsert_edge(&raw_edge(
            "b",
            "a",
            RelationType::Modified,
            1.0,
            1.0,
            NOW - 10 * HOUR,
        ))
        .unwrap();

        let (nodes, active_edges) = s.counts(NOW).unwrap();
        assert_eq!(nodes, 2);
        assert_eq!(active_edges, 1); // decayed b->a excluded
    }

    #[test]
    fn list_agents_reports_observation_counts() {
        let s = store();
        add_node(&s, "agent:a", NodeType::Agent, "a", NOW);
        add_node(&s, "artifact:x", NodeType::Artifact, "x", NOW);
        s.upsert_edge(&raw_edge(
            "agent:a",
            "artifact:x",
            RelationType::Observed,
            1.0,
            48.0,
            NOW,
        ))
        .unwrap();
        let agents = s.list_agents(NOW).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].id, "agent:a");
        assert_eq!(agents[0].observations, 1);
    }

    #[test]
    fn build_fts_query_sanitizes_input() {
        assert_eq!(build_fts_query("hello world!"), "\"hello\" OR \"world\"");
        assert_eq!(build_fts_query("a b"), ""); // all terms too short
        assert_eq!(build_fts_query("session"), "\"session\"");
    }
}
