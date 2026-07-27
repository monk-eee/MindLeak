//! Scope matching: whether a declared scope covers a concrete target.
//!
//! Two vocabularies share one matcher. A **code scope** names MindLeak nodes
//! (`artifact:crates/lodestar-core/src/lib.rs`, or `artifact:crates/**`). A
//! **workflow scope** names a procedural action (`workflow:git.publish`)
//! rather than a file, because some rules govern what an agent *did* and no
//! code binding can express that (ADR-0034).
//!
//! Both clauses and waivers declare scope, so the matcher lives here rather
//! than inside either one.

/// Scope tokens naming a procedural action rather than a code node (ADR-0034).
pub const WORKFLOW_PREFIX: &str = "workflow:";

/// Whether a scope token names an action rather than a code node.
pub fn is_workflow(scope: &str) -> bool {
    scope.starts_with(WORKFLOW_PREFIX)
}

/// Whether a declared workflow scope governs an intended action.
///
/// A parent token governs its children (`workflow:git` covers
/// `workflow:git.publish`) so a project can write one broad rule, but never the
/// reverse — otherwise a narrow clause about publishing would silently claim
/// authority over every git action.
pub fn workflow_governs(declared: &str, intended: &str) -> bool {
    intended == declared
        || intended
            .strip_prefix(declared)
            .is_some_and(|rest| rest.starts_with('.'))
}

/// Whether a declared scope covers a concrete target.
///
/// Deliberately not a glob engine: exact match, or a trailing `**` covering
/// everything beneath a prefix. A richer pattern language would let a waiver
/// author write a scope whose reach is hard to read, and the whole point of a
/// scoped exception is that a reviewer can see how far it goes.
pub fn covers(declared: &str, target: &str) -> bool {
    if declared == target {
        return true;
    }
    if is_workflow(declared) {
        return workflow_governs(declared, target);
    }
    declared
        .strip_suffix("**")
        .is_some_and(|prefix| target.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_parent_workflow_token_governs_its_children_but_not_the_reverse() {
        assert!(workflow_governs("workflow:git", "workflow:git.publish"));
        assert!(workflow_governs("workflow:git", "workflow:git"));
        assert!(!workflow_governs("workflow:git.publish", "workflow:git"));
        // A prefix that is not a token boundary is a different action.
        assert!(!workflow_governs("workflow:git", "workflow:github"));
    }

    #[test]
    fn a_recursive_scope_covers_everything_beneath_its_prefix() {
        assert!(covers(
            "artifact:crates/**",
            "artifact:crates/lodestar-core/src/lib.rs"
        ));
        assert!(!covers("artifact:crates/**", "artifact:docs/SPEC.md"));
        // A sibling directory sharing a prefix is not covered.
        assert!(!covers("artifact:crates/**", "artifact:cratesX/lib.rs"));
    }

    #[test]
    fn an_exact_scope_covers_only_itself() {
        assert!(covers("artifact:README.md", "artifact:README.md"));
        assert!(!covers("artifact:README.md", "artifact:README.md.bak"));
    }
}
