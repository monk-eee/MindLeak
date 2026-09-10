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

    /// Correct stored attribution using a trusted repository reader, never caller-supplied facts.
    pub fn repair_commit_attribution_for_agent(
        &self,
        agent: &str,
        sha: &str,
        reason: &str,
        read_commit: impl FnOnce(&str, &str) -> Result<CommitRecord>,
    ) -> Result<crate::CommitRepairOutcome> {
        let sha = sha.trim().to_ascii_lowercase();
        let reason = reason.trim();
        if !ingest::git::is_full_commit_sha(&sha) {
            return Err(crate::MindLeakError::InvalidArgument(
                "commit repair requires a full commit hash".into(),
            ));
        }
        if agent.trim().is_empty() || reason.is_empty() || reason.len() > 2048 {
            return Err(crate::MindLeakError::InvalidArgument(
                "commit repair requires an agent and a nonblank reason of at most 2048 bytes"
                    .into(),
            ));
        }
        let root = self.workspace_root.as_deref().ok_or_else(|| {
            crate::MindLeakError::InvalidArgument(
                "commit repair requires a configured repository workspace".into(),
            )
        })?;
        let verified = read_commit(root, &sha)?;
        if verified.sha.as_deref() != Some(sha.as_str()) {
            return Err(crate::MindLeakError::InvalidArgument(
                "Git returned a different commit than requested; nothing repaired".into(),
            ));
        }
        let roots = self.roots();
        self.store.repair_commit_attribution(
            agent.trim(),
            &verified,
            reason,
            now_unix(),
            &crate::borrowed(&roots),
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MindLeakError;

    fn repair_fixture() -> (MindLeak, CommitRecord) {
        let engine = MindLeak::open_in_memory()
            .unwrap()
            .with_workspace_root("/fixture");
        let verified = CommitRecord {
            sha: Some("a".repeat(40)),
            message: "fix: verified".into(),
            changed_files: vec!["correct.rs".into()],
            timestamp: 100,
        };
        engine
            .ingest_commit_for_agent(
                "author",
                &CommitRecord {
                    message: "wrong publication".into(),
                    changed_files: vec!["wrong.rs".into()],
                    timestamp: 900,
                    ..verified.clone()
                },
            )
            .unwrap();
        (engine, verified)
    }

    #[test]
    fn commit_repair_reads_the_configured_repository_without_stealing_authorship() {
        let (engine, verified) = repair_fixture();
        let sha = verified.sha.clone().unwrap();
        let outcome = engine
            .repair_commit_attribution_for_agent(
                "repairer",
                &sha.to_uppercase(),
                "wrong branch scope",
                |root, requested| {
                    assert_eq!(root, "/fixture");
                    assert_eq!(requested, sha);
                    Ok(verified)
                },
            )
            .unwrap();
        assert_eq!(outcome.removed_edges, 1);
        assert_eq!(outcome.added_edges, 1);
        assert!(outcome.audit_id.is_some());
        assert!(engine.store().get_node("agent:repairer").unwrap().is_none());
        assert!(engine.store().get_node("agent:author").unwrap().is_some());
    }

    #[test]
    fn commit_repair_refuses_unavailable_or_mismatched_git_facts_without_mutation() {
        let (engine, verified) = repair_fixture();
        let sha = verified.sha.clone().unwrap();
        for response in [
            Err(MindLeakError::Other("Git unavailable".into())),
            Ok(CommitRecord {
                sha: Some("b".repeat(40)),
                ..verified.clone()
            }),
        ] {
            assert!(engine
                .repair_commit_attribution_for_agent(
                    "repairer",
                    &sha,
                    "correct attribution",
                    |_, _| response
                )
                .is_err());
            assert_eq!(
                engine
                    .store()
                    .get_node(&format!("intent:{sha}"))
                    .unwrap()
                    .unwrap()
                    .created_at,
                900
            );
            assert!(engine
                .store()
                .get_node("artifact:correct.rs")
                .unwrap()
                .is_none());
        }
        assert_eq!(
            crate::telemetry::snapshot(&engine.store().conn, 10)
                .unwrap()
                .total_events,
            0
        );
    }

    #[test]
    fn invalid_commit_repair_requests_are_refused_before_reading_git() {
        let (engine, verified) = repair_fixture();
        let sha = verified.sha.unwrap();
        let oversized = "x".repeat(2049);
        for (agent, requested, reason) in [
            ("repairer", "HEAD", "reason"),
            ("", sha.as_str(), "reason"),
            ("repairer", sha.as_str(), " "),
            ("repairer", sha.as_str(), oversized.as_str()),
        ] {
            assert!(engine
                .repair_commit_attribution_for_agent(agent, requested, reason, |_, _| panic!(
                    "invalid requests must not read Git"
                ))
                .is_err());
        }
        let engine = engine.with_workspace_root("");
        assert!(engine
            .repair_commit_attribution_for_agent("repairer", &sha, "reason", |_, _| panic!(
                "an unconfigured workspace must not read Git"
            ))
            .is_err());
    }
}
