//! Which constitutional clauses govern a check, and how they resolve.
//!
//! Split out of `facade/conformance.rs` (see `super`); the code is unchanged.

use super::*;

impl Lodestar {
    /// Resolve which active clauses govern a set of intended or changed nodes,
    /// classified by how each relates to a covering task's own goal. This is the
    /// one place the constitution is read against a change scope, shared by
    /// retrospective conformance (`evaluate_base_conformance`) and the
    /// forward-looking `advise` (ADR-0029) so neither forks the rule. A
    /// documentation node contributes only a `forbid_change` lock; a `governed`
    /// binding to a doc is ignored at read time (a changelog touch must not
    /// drift), and no stored binding is mutated.
    pub(crate) fn resolve_governing_clauses(
        &self,
        node_ids: &[String],
        task_goal_id: Option<&str>,
    ) -> Result<GoverningClauses> {
        self.resolve_governing_clauses_covering(node_ids, task_goal_id, &[])
    }

    /// As [`Self::resolve_governing_clauses`], but also treating a binding to
    /// any goal in `covered` as in scope (ADR-0041). `covered` holds the
    /// additional goals a task declared at creation; the goals actually relied
    /// on are recorded so the caller can report which declarations mattered.
    pub(crate) fn resolve_governing_clauses_covering(
        &self,
        node_ids: &[String],
        task_goal_id: Option<&str>,
        covered: &[String],
    ) -> Result<GoverningClauses> {
        let mut resolved = GoverningClauses::default();
        for node in node_ids {
            let node_is_doc = is_documentation_node(node);
            for binding in self.store.active_bindings_for_node(node)? {
                if binding.mode == CodeBindingMode::ForbidChange {
                    resolved.forbid.push((node.clone(), binding.goal));
                    continue;
                }
                if node_is_doc {
                    continue;
                }
                match task_goal_id {
                    Some(goal_id) if binding.goal.id == goal_id => {
                        resolved.in_scope.push((node.clone(), binding.goal))
                    }
                    _ if covered.contains(&binding.goal.id) => {
                        resolved.relied_on_coverage.push(binding.goal.id.clone());
                        resolved.in_scope.push((node.clone(), binding.goal))
                    }
                    _ => resolved.other.push((node.clone(), binding.goal)),
                }
            }
        }
        resolved.relied_on_coverage.sort();
        resolved.relied_on_coverage.dedup();
        Ok(resolved)
    }

    /// Clauses governing an intended *procedural* action (ADR-0034).
    ///
    /// A workflow clause declares a `workflow:` scope instead of binding to code
    /// nodes, so it resolves by scope match rather than by binding lookup — a
    /// rule like "a protected branch advances only by reviewed merge" governs an
    /// action, and there is no artifact to bind it to.
    ///
    /// A parent token governs its children, so a clause scoped `workflow:git`
    /// covers an intent of `workflow:git.publish`. The reverse never holds: a
    /// clause about publishing does not govern every git action.
    pub(crate) fn resolve_workflow_clauses(
        &self,
        scopes: &[String],
    ) -> Result<Vec<(String, Goal)>> {
        if scopes.is_empty() {
            return Ok(Vec::new());
        }
        let mut resolved = Vec::new();
        for goal in self.store.goals_by_status(GoalStatus::Active)? {
            let Some(declared) = goal.scope.as_deref() else {
                continue;
            };
            if !scope::is_workflow(declared) {
                continue;
            }
            if let Some(intended) = scopes
                .iter()
                .find(|intended| scope::workflow_governs(declared, intended))
            {
                resolved.push((intended.clone(), goal));
            }
        }
        Ok(resolved)
    }
}
