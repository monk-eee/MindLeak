//! The conformance token: what a check was issued against.
//!
//! Split out of `facade/conformance.rs` (see `super`); the code is unchanged.

use super::*;

impl Lodestar {
    pub(super) fn conformance_token(
        &self,
        id: i64,
        evidence: &ConformanceEvidence,
        check: &ConformanceCheck,
        task: Option<&Task>,
    ) -> Result<String> {
        let mut basis = Vec::new();
        if let Some(task) = task {
            // `claim_lapses` belongs in the basis because the window start no
            // longer moves when the same owner re-claims (ADR-0048). Without it
            // a lease could lapse between a check and its completion and the
            // token would still match, letting a continuous-window verdict
            // certify a window that had since acquired a hole.
            basis.push(format!(
                "claim:{}:{}:{}:{}:{}",
                task.id,
                task.goal_id,
                task.owner.as_deref().unwrap_or_default(),
                task.claim_started_at.unwrap_or_default(),
                task.claim_lapses
            ));
            let goal = self
                .store
                .get_goal(&task.goal_id)?
                .ok_or_else(|| LodestarError::NotFound(task.goal_id.clone()))?;
            basis.push(format!("task-goal:{}", serde_json::to_string(&goal)?));
        }

        let mut nodes = evidence.changed_node_ids.clone();
        nodes.sort();
        nodes.dedup();
        for node in &nodes {
            for binding in self.store.active_bindings_for_node(node)? {
                basis.push(format!(
                    "binding:{node}:{}:{}",
                    binding.mode.as_str(),
                    serde_json::to_string(&binding.goal)?
                ));
            }
        }
        let changed: HashSet<&str> = nodes.iter().map(String::as_str).collect();
        for knowledge in self.store.active_knowledge(now_unix())? {
            if knowledge
                .referenced_nodes()
                .iter()
                .any(|node| changed.contains(node.as_str()))
            {
                basis.push(format!("knowledge:{}", serde_json::to_string(&knowledge)?));
            }
        }

        // Policy identity (ADR-0034). A token must not survive a change to the
        // constitution that authorised it, or to the controls that resolve its
        // clauses: a check issued under one policy is not evidence about
        // another. Recording the control *version* matters as much as its id,
        // because a redefined mechanism can reach a different verdict from the
        // same observation.
        if let Some(version) = self.store.active_constitution_version()? {
            basis.push(format!("constitution:{}:{}", version.id, version.version));
        }
        let mut clause_ids: Vec<String> = Vec::new();
        for node in &nodes {
            for binding in self.store.active_bindings_for_node(node)? {
                clause_ids.push(binding.goal.id);
            }
        }
        clause_ids.sort();
        clause_ids.dedup();
        for clause_id in &clause_ids {
            for control in self.store.controls_for_clause(clause_id)? {
                basis.push(format!(
                    "control:{}:{}:{}:{}",
                    control.id,
                    control.clause_id,
                    control.version,
                    control.power.as_str()
                ));
            }
            // Waiver state (SPEC-CONSTITUTION §9). A check made while an
            // exception was in force is not evidence about a world where it was
            // revoked, and one made under enforcement is not evidence about a
            // world where an exception was since granted. Recording `expires_at`
            // as well as status means a token also stops matching once the
            // waiver lapses — expiry restores enforcement without anyone
            // rewriting a row, so nothing else would notice.
            for waiver in self.store.waivers_for_clause(clause_id)? {
                basis.push(format!(
                    "waiver:{}:{}:{}:{}:{}",
                    waiver.id,
                    waiver.clause_id,
                    waiver.scope,
                    waiver.status.as_str(),
                    waiver.expires_at
                ));
            }
        }
        basis.sort();

        let mut hasher = Sha256::new();
        hasher.update(id.to_le_bytes());
        hasher.update(serde_json::to_vec(evidence)?);
        hasher.update(check.verdict.as_str().as_bytes());
        hasher.update(serde_json::to_vec(&check.findings)?);
        hasher.update(basis.join("\n").as_bytes());
        Ok(hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
}
