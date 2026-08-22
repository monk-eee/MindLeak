//! Artifact bindings: link and unlink goals to/from the MindLeak nodes that
//! realise them, and audit which goals currently govern a node.

use crate::{ArtifactBinding, ArtifactBindingMode, Lodestar, LodestarError, Result};

impl Lodestar {
    /// Bind a goal to the MindLeak nodes that realise it, so conformance can
    /// tell which intent governs an artefact.
    ///
    /// The verb binds *artefacts*, not only code (ADR-0060). A goal whose
    /// delivery is an ADR, documentation, a benchmark or a build script binds
    /// those, and a task against that goal can then answer for what it actually
    /// produced instead of reading as having touched nothing. Binding a
    /// documentation node does not make honest edits to it drift under some
    /// *other* goal — shared prose stays shared; see `is_documentation_node`.
    pub fn link_goal_to_artifact(
        &self,
        goal_id: &str,
        node_ids: &[String],
        mode: ArtifactBindingMode,
    ) -> Result<usize> {
        if !self.store.goal_exists(goal_id)? {
            return Err(LodestarError::NotFound(goal_id.to_string()));
        }
        let goal = self
            .store
            .get_goal(goal_id)?
            .ok_or_else(|| LodestarError::NotFound(goal_id.to_string()))?;
        if mode == ArtifactBindingMode::ForbidChange && !goal.kind.is_normative() {
            return Err(LodestarError::Invalid(
                "forbid_change is valid only for constraints and invariants".to_string(),
            ));
        }
        self.store.link_goal_to_artifact(goal_id, node_ids, mode)
    }

    /// Remove goal↔artifact bindings (ADR-0009 seam upkeep). The inverse of
    /// `link_goal_to_artifact`: prune a stale or mistaken binding — e.g. a shared doc
    /// or a source file that a goal no longer governs — so conformance stops
    /// flagging honest changes to it as drift against a goal it does not realise.
    /// A node not bound to the goal is a no-op. Returns how many bindings were
    /// removed.
    pub fn unlink_goal_from_artifact(&self, goal_id: &str, node_ids: &[String]) -> Result<usize> {
        if !self.store.goal_exists(goal_id)? {
            return Err(LodestarError::NotFound(goal_id.to_string()));
        }
        self.store.unlink_goal_from_artifact(goal_id, node_ids)
    }

    /// Audit which active goals govern a code node, and how (governed /
    /// forbid_change) — the read that makes binding hygiene inspectable before
    /// pruning with `unlink_goal_from_artifact`.
    pub fn governing_goals(&self, node_id: &str) -> Result<Vec<ArtifactBinding>> {
        self.store.active_bindings_for_node(node_id)
    }
}
