use crate::ingest::execution::ExecutionRecord;
use crate::ingest::git::CommitRecord;
use crate::ingest::tool_invocation::ToolInvocationRecord;
use crate::{ingest, now_unix, Edge, MindLeak, Node, NodeType, RelationType, Result, WriteOutcome};

mod file;
mod reconcile;

impl MindLeak {
    /// Record that one explicit session agent observed these nodes.
    pub(super) fn observe(&self, agent: &str, ids: &[String], now: i64) -> Result<()> {
        let agent = agent.trim().strip_prefix("agent:").unwrap_or(agent.trim());
        if agent.is_empty() {
            return Ok(());
        }
        let agent_id = format!("agent:{agent}");
        self.store
            .upsert_node(&Node::new(&agent_id, NodeType::Agent, agent, now))?;
        for id in ids {
            if id == &agent_id {
                continue;
            }
            let mut edge = Edge::new(&agent_id, id, RelationType::Observed, now);
            // Attribution of a transient execution must not outlive the
            // execution's own evidence. Left at the generic `observed` half-life
            // (48h) the attribution edge pins the execution in the graph for
            // roughly twice as long as its 24h `modified` evidence, so prune
            // cannot reap the spent execution until ~9 days out. Cap execution
            // attribution to the execution decay tier so both fade together and
            // the orphaned execution is reaped promptly (ADR-0021 / ADR-0003).
            if id.starts_with("execution:") || id.starts_with("tool_invocation:") {
                edge.half_life_hours = RelationType::Modified.default_half_life_hours();
            }
            self.store.upsert_edge(&edge)?;
        }
        Ok(())
    }

    // ---- ingestion ----------------------------------------------------------

    pub fn ingest_execution(&self, rec: &ExecutionRecord) -> Result<WriteOutcome> {
        let now = now_unix();
        let roots = self.roots();
        ingest::execution::ingest_execution(&self.store, rec, now, &crate::borrowed(&roots))
    }

    pub fn ingest_execution_for_agent(
        &self,
        agent: &str,
        rec: &ExecutionRecord,
    ) -> Result<WriteOutcome> {
        let now = now_unix();
        let roots = self.roots();
        let outcome =
            ingest::execution::ingest_execution(&self.store, rec, now, &crate::borrowed(&roots))?;
        self.observe(agent, &outcome.node_ids, now)?;
        Ok(outcome)
    }

    /// One ingest path for both entry points, reporting the `now` it used so
    /// attribution cannot drift from the edges it is attributing.
    fn ingest_commit_at(&self, rec: &CommitRecord) -> Result<(WriteOutcome, i64)> {
        let now = now_unix();
        let roots = self.roots();
        let roots = crate::borrowed(&roots);
        let outcome =
            ingest::git::ingest_commit(&self.store, rec, now, &roots, self.commit_resolver())?;
        Ok((outcome, now))
    }

    pub fn ingest_commit(&self, rec: &CommitRecord) -> Result<WriteOutcome> {
        Ok(self.ingest_commit_at(rec)?.0)
    }

    pub fn ingest_commit_for_agent(&self, agent: &str, rec: &CommitRecord) -> Result<WriteOutcome> {
        let (outcome, now) = self.ingest_commit_at(rec)?;
        self.observe(agent, &outcome.node_ids, now)?;
        Ok(outcome)
    }

    pub fn ingest_tool_invocation(&self, rec: &ToolInvocationRecord) -> Result<WriteOutcome> {
        ingest::tool_invocation::ingest_tool_invocation(&self.store, rec)
    }

    pub fn ingest_tool_invocation_for_agent(
        &self,
        agent: &str,
        rec: &ToolInvocationRecord,
    ) -> Result<WriteOutcome> {
        let now = now_unix();
        let outcome = ingest::tool_invocation::ingest_tool_invocation(&self.store, rec)?;
        self.observe(agent, &outcome.node_ids, now)?;
        Ok(outcome)
    }

    /// Record node attention for recency displays without rewriting evidence.
    pub fn boost(&self, id: &str) -> Result<bool> {
        self.store.boost(id, now_unix())
    }

    pub fn boost_for_agent(&self, agent: &str, id: &str) -> Result<bool> {
        let now = now_unix();
        let boosted = self.store.boost(id, now)?;
        if boosted {
            self.observe(agent, &[id.to_string()], now)?;
        }
        Ok(boosted)
    }
}
