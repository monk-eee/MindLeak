//! Shape and window validation for a submitted evidence bundle.
//!
//! Split out of `facade/conformance.rs` (see `super`); the code is unchanged.

use super::*;

impl Lodestar {
    pub(super) fn validate_evidence_shape(&self, evidence: &ConformanceEvidence) -> Result<()> {
        if evidence.schema_version != 1 {
            return Err(LodestarError::Invalid(format!(
                "unsupported evidence schema version {}",
                evidence.schema_version
            )));
        }
        if evidence.started_at > evidence.ended_at {
            return Err(LodestarError::Invalid(
                "evidence start must not be after its end".to_string(),
            ));
        }
        if evidence.agent_id.trim().is_empty() {
            return Err(LodestarError::Invalid(
                "evidence agent must not be empty".to_string(),
            ));
        }
        if evidence.execution_ids.len() > MAX_EVIDENCE_EVENTS
            || evidence.commit_ids.len() > MAX_EVIDENCE_EVENTS
            || evidence.provenance.len() > MAX_EVIDENCE_PROVENANCE
            || evidence.summary.len() > MAX_EVIDENCE_SUMMARY_BYTES
        {
            return Err(LodestarError::Invalid(
                "evidence exceeds the bounded ADR-0009 contract".to_string(),
            ));
        }
        if !evidence
            .successful_execution_ids
            .iter()
            .all(|id| evidence.execution_ids.contains(id))
        {
            return Err(LodestarError::Invalid(
                "successful executions must be included in execution_ids".to_string(),
            ));
        }
        let agent_node_id = format!("agent:{}", evidence.agent_id);
        for event_id in evidence
            .execution_ids
            .iter()
            .chain(evidence.commit_ids.iter())
        {
            if !evidence.provenance.iter().any(|fact| {
                fact.source_id == agent_node_id
                    && fact.target_id == *event_id
                    && fact.relation == "observed"
            }) {
                return Err(LodestarError::Invalid(format!(
                    "event {event_id} lacks agent observation provenance"
                )));
            }
        }
        for changed_id in &evidence.changed_node_ids {
            if !evidence.provenance.iter().any(|fact| {
                fact.target_id == *changed_id
                    && matches!(fact.relation.as_str(), "modified" | "refactored")
                    && (evidence.execution_ids.contains(&fact.source_id)
                        || evidence.commit_ids.contains(&fact.source_id))
            }) {
                return Err(LodestarError::Invalid(format!(
                    "changed node {changed_id} lacks mutation provenance"
                )));
            }
        }
        for failed_id in &evidence.failed_node_ids {
            if !evidence.provenance.iter().any(|fact| {
                fact.target_id == *failed_id
                    && fact.relation == "failed_on"
                    && evidence.execution_ids.contains(&fact.source_id)
            }) {
                return Err(LodestarError::Invalid(format!(
                    "failed node {failed_id} lacks failure provenance"
                )));
            }
        }
        Ok(())
    }

    pub(super) fn validate_claim_evidence(
        &self,
        task: &Task,
        agent: &str,
        evidence: &ConformanceEvidence,
        now: i64,
    ) -> Result<()> {
        self.validate_evidence_shape(evidence)?;
        if evidence.task_id.as_deref() != Some(task.id.as_str()) {
            return Err(LodestarError::Invalid(
                "evidence task_id does not identify the claimed task".to_string(),
            ));
        }
        if evidence.agent_id != agent || task.owner.as_deref() != Some(agent) {
            return Err(LodestarError::Invalid(
                "evidence agent does not own the task".to_string(),
            ));
        }
        let lease_expired = match task.lease_expires_at {
            Some(end) => end < now,
            None => true,
        };
        if task.status != TaskStatus::Claimed || lease_expired {
            return Err(LodestarError::Invalid(
                "task does not have a live claim".to_string(),
            ));
        }
        let claim_started_at = task.claim_started_at.ok_or_else(|| {
            LodestarError::Invalid("task claim has no evidence-window start".to_string())
        })?;
        // The floor is the window that *authorised* the work, not the latest one
        // opened over it. A recovery exists to rescue work already done, so it
        // always opens its window after that work happened; comparing against
        // the live window alone meant a recovered claim could never certify
        // anything, whatever order its owner called things in. Walking the
        // audited recovery chain keeps the guarantee intact — the floor is still
        // the start of a real claim, held by an identity this one demonstrably
        // took the task from (ADR-0030) — while no longer refusing the evidence
        // the recovery was performed to rescue.
        let authorising_start = self
            .store
            .authorising_window_start(&task.id, agent)?
            .unwrap_or(claim_started_at)
            .min(claim_started_at);
        if evidence.started_at < authorising_start
            || evidence.ended_at > now
            || evidence.ended_at > task.lease_expires_at.unwrap_or(evidence.ended_at)
        {
            return Err(LodestarError::Invalid(
                "evidence interval falls outside the live claim".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facade::test_support::engine;
    use crate::EvidenceProvenance;

    fn valid_evidence(task_id: &str, agent: &str, now: i64) -> ConformanceEvidence {
        ConformanceEvidence {
            schema_version: 1,
            task_id: Some(task_id.into()),
            agent_id: agent.into(),
            started_at: now - 5,
            ended_at: now,
            changed_node_ids: vec!["artifact:src/lib.rs".into()],
            failed_node_ids: Vec::new(),
            execution_ids: vec!["execution:proof".into()],
            successful_execution_ids: vec!["execution:proof".into()],
            commit_ids: Vec::new(),
            ledger_act_ids: Vec::new(),
            summary: "changed lib.rs".into(),
            provenance: vec![
                EvidenceProvenance {
                    source_id: format!("agent:{agent}"),
                    target_id: "execution:proof".into(),
                    relation: "observed".into(),
                },
                EvidenceProvenance {
                    source_id: "execution:proof".into(),
                    target_id: "artifact:src/lib.rs".into(),
                    relation: "modified".into(),
                },
            ],
        }
    }

    /// A task that is claimed, live, and never inserted into the store — a
    /// nonexistent task_id makes `authorising_window_start` return `Ok(None)`,
    /// so `claim_started_at` alone governs the floor.
    fn claimed_task(id: &str, agent: &str, now: i64) -> Task {
        Task {
            id: id.into(),
            goal_id: "goal:test".into(),
            parent_task_id: None,
            title: "test task".into(),
            acceptance: "test acceptance".into(),
            status: TaskStatus::Claimed,
            owner: Some(agent.into()),
            claim_started_at: Some(now - 10),
            lease_expires_at: Some(now + 300),
            blocked_by: None,
            branch: None,
            parked_at: None,
            resolved_by: None,
            resolved_at: None,
            resolved_conformance_id: None,
            created_at: now - 100,
            updated_at: now - 10,
        }
    }

    #[test]
    fn valid_evidence_against_a_live_claim_is_accepted() {
        let e = engine();
        let now = now_unix();
        let task = claimed_task("task:real", "agent-a", now);
        let evidence = valid_evidence(&task.id, "agent-a", now);
        e.validate_claim_evidence(&task, "agent-a", &evidence, now)
            .unwrap();
    }

    #[test]
    fn unsupported_schema_version_is_rejected() {
        let e = engine();
        let mut evidence = valid_evidence("task:real", "agent-a", now_unix());
        evidence.schema_version = 2;
        let err = e.validate_evidence_shape(&evidence).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: unsupported evidence schema version 2"
        );
    }

    #[test]
    fn reversed_evidence_window_is_rejected() {
        let e = engine();
        let mut evidence = valid_evidence("task:real", "agent-a", now_unix());
        evidence.started_at = 10;
        evidence.ended_at = 5;
        let err = e.validate_evidence_shape(&evidence).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: evidence start must not be after its end"
        );
    }

    #[test]
    fn empty_agent_id_is_rejected() {
        let e = engine();
        let mut evidence = valid_evidence("task:real", "agent-a", now_unix());
        evidence.agent_id = "   ".into();
        let err = e.validate_evidence_shape(&evidence).unwrap_err();
        assert_eq!(err.to_string(), "invalid: evidence agent must not be empty");
    }

    #[test]
    fn oversized_evidence_is_rejected() {
        let e = engine();
        let mut evidence = valid_evidence("task:real", "agent-a", now_unix());
        evidence.execution_ids = (0..=MAX_EVIDENCE_EVENTS)
            .map(|i| format!("execution:{i}"))
            .collect();
        let err = e.validate_evidence_shape(&evidence).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: evidence exceeds the bounded ADR-0009 contract"
        );
    }

    #[test]
    fn successful_execution_absent_from_execution_ids_is_rejected() {
        let e = engine();
        let mut evidence = valid_evidence("task:real", "agent-a", now_unix());
        evidence.successful_execution_ids = vec!["execution:never-attempted".into()];
        let err = e.validate_evidence_shape(&evidence).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: successful executions must be included in execution_ids"
        );
    }

    #[test]
    fn execution_missing_observed_provenance_is_rejected() {
        let e = engine();
        let mut evidence = valid_evidence("task:real", "agent-a", now_unix());
        evidence
            .provenance
            .retain(|fact| fact.relation != "observed");
        let err = e.validate_evidence_shape(&evidence).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: event execution:proof lacks agent observation provenance"
        );
    }

    #[test]
    fn changed_node_missing_mutation_provenance_is_rejected() {
        let e = engine();
        let mut evidence = valid_evidence("task:real", "agent-a", now_unix());
        // Matches target and relation, but its source is in neither
        // execution_ids nor commit_ids -- still not mutation provenance.
        evidence
            .provenance
            .retain(|fact| fact.relation != "modified");
        evidence.provenance.push(EvidenceProvenance {
            source_id: "execution:untracked".into(),
            target_id: "artifact:src/lib.rs".into(),
            relation: "modified".into(),
        });
        let err = e.validate_evidence_shape(&evidence).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: changed node artifact:src/lib.rs lacks mutation provenance"
        );
    }

    #[test]
    fn failed_node_missing_failure_provenance_is_rejected() {
        let e = engine();
        let mut evidence = valid_evidence("task:real", "agent-a", now_unix());
        evidence.failed_node_ids = vec!["execution:broken".into()];
        // Matches target and relation, but its source is not an execution_id
        // -- still not failure provenance.
        evidence.provenance.push(EvidenceProvenance {
            source_id: "commit:untracked".into(),
            target_id: "execution:broken".into(),
            relation: "failed_on".into(),
        });
        let err = e.validate_evidence_shape(&evidence).unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: failed node execution:broken lacks failure provenance"
        );
    }

    #[test]
    fn failed_node_with_real_failure_provenance_is_accepted() {
        let e = engine();
        let mut evidence = valid_evidence("task:real", "agent-a", now_unix());
        evidence.failed_node_ids = vec!["execution:broken".into()];
        evidence.provenance.push(EvidenceProvenance {
            source_id: "execution:proof".into(),
            target_id: "execution:broken".into(),
            relation: "failed_on".into(),
        });
        e.validate_evidence_shape(&evidence).unwrap();
    }

    #[test]
    fn evidence_task_id_mismatch_is_rejected() {
        let e = engine();
        let now = now_unix();
        let task = claimed_task("task:real", "agent-a", now);
        let evidence = valid_evidence("task:other", "agent-a", now);
        let err = e
            .validate_claim_evidence(&task, "agent-a", &evidence, now)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: evidence task_id does not identify the claimed task"
        );
    }

    #[test]
    fn evidence_agent_id_not_matching_the_caller_is_rejected() {
        let e = engine();
        let now = now_unix();
        let task = claimed_task("task:real", "agent-a", now);
        let evidence = valid_evidence(&task.id, "agent-b", now);
        let err = e
            .validate_claim_evidence(&task, "agent-a", &evidence, now)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: evidence agent does not own the task"
        );
    }

    #[test]
    fn task_owner_not_matching_the_caller_is_rejected() {
        let e = engine();
        let now = now_unix();
        let mut task = claimed_task("task:real", "agent-a", now);
        task.owner = Some("agent-b".into());
        let evidence = valid_evidence(&task.id, "agent-a", now);
        let err = e
            .validate_claim_evidence(&task, "agent-a", &evidence, now)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: evidence agent does not own the task"
        );
    }

    #[test]
    fn task_with_no_lease_at_all_is_rejected() {
        let e = engine();
        let now = now_unix();
        let mut task = claimed_task("task:real", "agent-a", now);
        task.lease_expires_at = None;
        let evidence = valid_evidence(&task.id, "agent-a", now);
        let err = e
            .validate_claim_evidence(&task, "agent-a", &evidence, now)
            .unwrap_err();
        assert_eq!(err.to_string(), "invalid: task does not have a live claim");
    }

    #[test]
    fn task_not_in_the_claimed_status_is_rejected() {
        let e = engine();
        let now = now_unix();
        let mut task = claimed_task("task:real", "agent-a", now);
        task.status = TaskStatus::InReview;
        let evidence = valid_evidence(&task.id, "agent-a", now);
        let err = e
            .validate_claim_evidence(&task, "agent-a", &evidence, now)
            .unwrap_err();
        assert_eq!(err.to_string(), "invalid: task does not have a live claim");
    }

    #[test]
    fn claim_with_no_evidence_window_start_is_rejected() {
        let e = engine();
        let now = now_unix();
        let mut task = claimed_task("task:real", "agent-a", now);
        task.claim_started_at = None;
        let evidence = valid_evidence(&task.id, "agent-a", now);
        let err = e
            .validate_claim_evidence(&task, "agent-a", &evidence, now)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid: task claim has no evidence-window start"
        );
    }
}
