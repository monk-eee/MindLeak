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
        if evidence.started_at < claim_started_at
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
