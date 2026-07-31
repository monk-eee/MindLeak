//! Worktree discovery, so a path from a sibling checkout can be placed.

use std::path::Path;

use super::fs::git_command;

/// Every worktree root of the repository `workspace` belongs to, forward-slashed.
///
/// All worktrees of one repository share a single graph (ADR-0038), so a file
/// saved in any of them is the same file with one repo-relative identity.
/// Knowing every root is what lets a path from a sibling checkout be placed
/// instead of refused: measured 2026-07-30, 203 of 291 ingest calls were refused
/// because the path came from a worktree that was not the server's own.
///
/// Returns empty when git cannot answer. The caller then keeps whatever single
/// root it already had, so an unavailable git degrades to the previous behaviour
/// rather than to a wrong answer.
pub fn worktree_roots(workspace: &Path) -> Vec<String> {
    let Ok(output) = git_command()
        .args(["worktree", "list", "--porcelain"])
        .current_dir(workspace)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(|root| root.trim().replace('\\', "/"))
        .filter(|root| !root.is_empty())
        .collect()
}
