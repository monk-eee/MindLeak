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
    /// documentation node contributes a `forbid_change` lock, and a `governed`
    /// binding to one counts as in scope for the goal it was bound to — that
    /// binding is an explicit statement that this goal delivers that artefact
    /// (ADR-0060). It is ignored only as *drift*, so a changelog touch under
    /// some unrelated goal still does not fail. No stored binding is mutated.
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
        // Compare goals by slug, the identity a clause keeps across versions. A
        // clause carried into a new constitution is re-issued as
        // `goal:<slug>@constitution:vN` while a task still names the bare
        // `goal:<slug>`, so an equality test on the ids stops matching at the
        // first amendment — and every task touching governed code then reads as
        // unsanctioned, however correct it is.
        let task_slug = task_goal_id.map(goal_slug);
        for node in node_ids {
            let node_is_doc = is_documentation_node(node);
            for binding in self.store.active_bindings_for_node(node)? {
                if binding.mode == CodeBindingMode::ForbidChange {
                    resolved.forbid.push((node.clone(), binding.goal));
                    continue;
                }
                match task_slug {
                    Some(slug) if binding.goal.slug == slug => {
                        resolved.in_scope.push((node.clone(), binding.goal))
                    }
                    _ if covered.iter().any(|c| goal_slug(c) == binding.goal.slug) => {
                        resolved.relied_on_coverage.push(binding.goal.id.clone());
                        resolved.in_scope.push((node.clone(), binding.goal))
                    }
                    // A documentation binding to some *other* goal is not drift
                    // (ADR-0060). Shared prose is touched by everyone, so
                    // drifting on it would make CHANGELOG.md uneditable without
                    // a covering task — the case this exclusion was written for.
                    // It stays excluded here, and only here: a doc bound to the
                    // task's own goal, or to a goal the task declared it covers,
                    // is an explicit statement that this work delivers that
                    // artefact, and answering `touched_task_goal` with it is the
                    // whole point of binding it.
                    _ if node_is_doc => continue,
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
