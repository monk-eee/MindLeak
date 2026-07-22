//! MindLeak — Temporal Context Graph Engine (TCGE) core.
//!
//! A decay-weighted, directional knowledge graph for coding agents. Raw
//! telemetry (executions, commits, file symbols) is ingested deterministically
//! (zero LLM tokens) into nodes and edges; edge weights decay on an exponential
//! half-life so stale context fades out of query results. An optional local
//! Ollama worker consolidates noisy logs into high-level intent nodes.
//!
//! The three agent-facing operations mirror the MCP tool surface:
//! [`MindLeak::multi_hop_query`], [`MindLeak::impact_radius`], and
//! [`MindLeak::record_decision`].

pub mod consolidate;
pub mod db;
pub mod decay;
pub mod error;
pub mod graph;
pub mod ingest;
pub mod model;

use std::time::{SystemTime, UNIX_EPOCH};

pub use error::{MindLeakError, Result};
pub use graph::{
    AgentActivity, Direction, GraphStore, ScoredNode, Subgraph, WeightedEdge, WriteOutcome,
};
pub use model::{Edge, Node, NodeType, RelationType};

use consolidate::Consolidator;
use ingest::execution::ExecutionRecord;
use ingest::git::CommitRecord;

/// Current unix time in whole seconds.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// High-level facade over the graph store and the ingestion pipeline.
pub struct MindLeak {
    store: GraphStore,
    agent: Option<String>,
}

impl MindLeak {
    /// Open (or create) a MindLeak database at `path`.
    pub fn open(path: &str) -> Result<Self> {
        Ok(MindLeak {
            store: GraphStore::new(db::open(path)?),
            agent: None,
        })
    }

    /// Open an ephemeral in-memory graph (tests / tooling).
    pub fn open_in_memory() -> Result<Self> {
        Ok(MindLeak {
            store: GraphStore::new(db::open_in_memory()?),
            agent: None,
        })
    }

    /// Attach an agent id so ingest/focus operations also record decay-weighted
    /// `observed` edges (attribution). `None` or empty disables attribution.
    pub fn with_agent(mut self, agent: Option<String>) -> Self {
        self.agent = agent.filter(|a| !a.trim().is_empty());
        self
    }

    pub fn store(&self) -> &GraphStore {
        &self.store
    }

    /// Record that the active agent (if any) observed these nodes.
    fn observe(&self, ids: &[String], now: i64) -> Result<()> {
        let Some(agent) = &self.agent else {
            return Ok(());
        };
        let agent_id = format!("agent:{agent}");
        self.store
            .upsert_node(&Node::new(&agent_id, NodeType::Agent, agent.clone(), now))?;
        for id in ids {
            if id != &agent_id {
                self.store
                    .upsert_edge(&Edge::new(&agent_id, id, RelationType::Observed, now))?;
            }
        }
        Ok(())
    }

    // ---- ingestion ----------------------------------------------------------

    pub fn ingest_execution(&self, rec: &ExecutionRecord) -> Result<WriteOutcome> {
        let now = now_unix();
        let outcome = ingest::execution::ingest_execution(&self.store, rec, now)?;
        self.observe(&outcome.node_ids, now)?;
        Ok(outcome)
    }

    pub fn ingest_commit(&self, rec: &CommitRecord) -> Result<WriteOutcome> {
        let now = now_unix();
        let outcome = ingest::git::ingest_commit(&self.store, rec, now)?;
        self.observe(&outcome.node_ids, now)?;
        Ok(outcome)
    }

    /// Ingest a source file: an artifact node plus its extracted symbols,
    /// linked with `contains` edges.
    pub fn ingest_file(&self, path: &str, content: &str) -> Result<WriteOutcome> {
        let now = now_unix();
        let mut outcome = WriteOutcome::default();
        let norm = ingest::normalize_path(path);
        let art_id = format!("artifact:{norm}");
        let art = Node::new(&art_id, NodeType::Artifact, norm.clone(), now);
        if self.store.upsert_node(&art)? {
            outcome.nodes_created += 1;
        }
        outcome.node_ids.push(art_id.clone());

        let extraction = ingest::ast::extract(path, content);
        for sym in &extraction.symbols {
            let sym_id = format!("symbol:{norm}:{}", sym.name);
            let label = format!("{} ({})", sym.name, sym.kind);
            let node = Node::new(&sym_id, NodeType::Symbol, label, now)
                .with_content(format!("{}:{}", norm, sym.line));
            if self.store.upsert_node(&node)? {
                outcome.nodes_created += 1;
            }
            let edge = Edge::new(&art_id, &sym_id, RelationType::Contains, now);
            if self.store.upsert_edge(&edge)? {
                outcome.edges_created += 1;
            }
        }

        // In-file call edges (symbol -> symbol); both endpoints exist as nodes.
        for call in &extraction.calls {
            let from = format!("symbol:{norm}:{}", call.caller);
            let to = format!("symbol:{norm}:{}", call.callee);
            let edge = Edge::new(&from, &to, RelationType::Calls, now);
            if self.store.upsert_edge(&edge)? {
                outcome.edges_created += 1;
            }
        }
        self.observe(&outcome.node_ids, now)?;
        Ok(outcome)
    }

    /// Elevate a node (editor focus) so its context ranks higher immediately.
    pub fn boost(&self, id: &str) -> Result<bool> {
        let now = now_unix();
        let boosted = self.store.boost(id, now)?;
        if boosted {
            self.observe(&[id.to_string()], now)?;
        }
        Ok(boosted)
    }

    // ---- agent-facing queries (MCP tool surface) ----------------------------

    /// Tool 1: navigate the graph up to `max_depth` hops from a seed node or
    /// search phrase, keeping only edges above `min_weight` effective weight.
    pub fn multi_hop_query(
        &self,
        seed_entity: &str,
        max_depth: u32,
        min_weight: f64,
    ) -> Result<Subgraph> {
        let now = now_unix();
        let seeds = self.store.resolve_seed(seed_entity, 3)?;
        if seeds.is_empty() {
            return Ok(Subgraph {
                seed: vec![],
                nodes: vec![],
                edges: vec![],
            });
        }
        self.store
            .traverse(&seeds, Direction::Outgoing, max_depth, min_weight, now)
    }

    /// Tool 2: what changes/breaks if `target_artifact` is modified — dependents,
    /// prior failing executions, and related intents (bidirectional, depth 2).
    pub fn impact_radius(&self, target_artifact: &str) -> Result<Subgraph> {
        let now = now_unix();
        let seeds = self.store.resolve_seed(target_artifact, 1)?;
        if seeds.is_empty() {
            return Err(MindLeakError::NotFound(target_artifact.to_string()));
        }
        self.store.traverse(&seeds, Direction::Both, 2, 0.1, now)
    }

    /// Tool 3: record an explicit architectural decision as an intent node,
    /// linked to the nodes it affects.
    pub fn record_decision(
        &self,
        decision_text: &str,
        related_nodes: &[String],
    ) -> Result<(String, WriteOutcome)> {
        let now = now_unix();
        let mut outcome = WriteOutcome::default();
        let intent_id = format!("intent:{}", ingest::short_hash(decision_text));
        let label = ingest::clamp(decision_text, 80);
        let intent =
            Node::new(&intent_id, NodeType::Intent, label, now).with_content(decision_text);
        if self.store.upsert_node(&intent)? {
            outcome.nodes_created += 1;
        }
        outcome.node_ids.push(intent_id.clone());

        for target in related_nodes {
            if self.store.node_exists(target)? {
                let edge = Edge::new(&intent_id, target, RelationType::RelatesTo, now);
                if self.store.upsert_edge(&edge)? {
                    outcome.edges_created += 1;
                }
            }
        }
        self.observe(&outcome.node_ids, now)?;
        Ok((intent_id, outcome))
    }

    // ---- maintenance --------------------------------------------------------

    pub fn prune(&self) -> Result<(usize, usize)> {
        self.store.prune(now_unix())
    }

    pub fn counts(&self) -> Result<(i64, i64)> {
        self.store.counts(now_unix())
    }

    /// The agent roster: `agent` nodes with their active observation counts.
    pub fn list_agents(&self) -> Result<Vec<AgentActivity>> {
        self.store.list_agents(now_unix())
    }

    /// A visualization snapshot: either the neighbourhood of `seed` (both
    /// directions, depth 2) or the most recently accessed nodes when no seed.
    pub fn snapshot(&self, seed: Option<&str>, limit: usize) -> Result<Subgraph> {
        let now = now_unix();
        match seed {
            Some(s) if !s.is_empty() => {
                let seeds = self.store.resolve_seed(s, 1)?;
                if seeds.is_empty() {
                    self.store.snapshot(limit, now)
                } else {
                    self.store.traverse(&seeds, Direction::Both, 2, 0.05, now)
                }
            }
            _ => self.store.snapshot(limit, now),
        }
    }

    /// Optional: consolidate a batch of raw execution logs into a single intent
    /// node via a local, OpenAI-compatible model (`MINDLEAK_LLM_URL` /
    /// `MINDLEAK_MODEL`). This is the only path that calls an LLM, and it is
    /// never on the write/query hot path — it errors cleanly if no model is
    /// reachable.
    pub fn consolidate_session(&self, logs: &[String]) -> Result<(String, WriteOutcome)> {
        Consolidator::default().consolidate_and_store(&self.store, logs, now_unix())
    }
}
