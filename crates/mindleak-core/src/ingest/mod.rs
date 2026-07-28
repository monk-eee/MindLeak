//! Zero-token deterministic ingestion: turn raw telemetry into graph triples
//! using pattern matching only (no LLM tokens on the write path).

pub mod ast;
pub mod execution;
pub mod git;
pub(crate) mod javascript;
pub mod manifest;
pub mod structure;

use sha2::{Digest, Sha256};

/// Short stable hash used to build deterministic node ids.
pub(crate) fn short_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
    hex
}

/// Normalise a filesystem path to forward slashes for stable node ids.
pub(crate) fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

/// Normalise a path *and* make it relative to `root` when it sits inside it.
///
/// Node ids are repo-relative by contract. Editor sensors report absolute
/// paths, and every worktree of a repository shares one graph (ADR-0038), so an
/// absolute id splits a single file across as many identities as there are
/// checkouts -- fragmenting its history, its reinforcement, and its governance.
///
/// A path outside `root`, or a root that was never declared, is returned
/// normalised but otherwise untouched: guessing a relative form for something
/// that is genuinely elsewhere would invent a file that does not exist.
pub(crate) fn repo_relative(path: &str, root: Option<&str>) -> String {
    let normalized = normalize_path(path);
    let Some(root) = root.map(normalize_path) else {
        return normalized;
    };
    let root = root.trim_end_matches('/');
    if root.is_empty() {
        return normalized;
    }
    // Windows paths are case-insensitive and arrive with either drive casing,
    // so compare case-insensitively while keeping the caller's own spelling.
    let candidate = normalized.to_ascii_lowercase();
    let prefix = format!("{}/", root.to_ascii_lowercase());
    match candidate.strip_prefix(&prefix) {
        Some(_) => normalized[prefix.len()..].to_string(),
        None => normalized,
    }
}

/// Directory segments that never belong in a code-context graph: VCS internals,
/// dependency caches, and build/test output. They are regenerated or deleted
/// constantly, so ingesting them (via a passive save sensor, or a build/git
/// command's changed-files) only pollutes the structural tier with stale nodes
/// for paths that vanish. They are rejected on the deterministic write path.
const IGNORED_SEGMENTS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "dist",
    "coverage",
    ".mindleak",
    ".lodestar",
    ".vscode-test",
];

/// True when a path lives under a directory that should never be ingested.
/// Matches a junk directory in any position (e.g. `crates/x/target/y.json`).
pub(crate) fn is_ignored_path(path: &str) -> bool {
    normalize_path(path)
        .split('/')
        .any(|segment| IGNORED_SEGMENTS.contains(&segment))
}

/// Truncate a string to `max` chars (char-safe), appending an ellipsis marker.
pub(crate) fn clamp(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push_str(" …");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_paths_inside_the_checkout_become_repo_relative() {
        // The regression: every worktree of one repository shares a graph, so an
        // absolute id splits a file across as many identities as there are
        // checkouts. Measured before the fix: 871 absolute ids across 7
        // worktrees, 590 files living under two identities at once.
        let build = Some("C:/Users/lyndonswan/Repos/MindLeak-build");
        let gap = Some("C:/Users/lyndonswan/Repos/MindLeak-gap");

        assert_eq!(
            repo_relative("C:/Users/lyndonswan/Repos/MindLeak-build/AGENTS.md", build),
            "AGENTS.md"
        );
        // Two different checkouts must resolve to the same node id.
        assert_eq!(
            repo_relative("C:\\Users\\lyndonswan\\Repos\\MindLeak-gap\\AGENTS.md", gap),
            repo_relative("C:/Users/lyndonswan/Repos/MindLeak-build/AGENTS.md", build)
        );
    }

    #[test]
    fn repo_relative_leaves_alone_what_it_cannot_place() {
        let root = Some("C:/Users/lyndonswan/Repos/MindLeak-build");

        // Already relative: untouched apart from separators.
        assert_eq!(repo_relative("crates\\x\\lib.rs", root), "crates/x/lib.rs");
        // Genuinely outside the checkout: inventing a relative form would name a
        // file that does not exist.
        assert_eq!(
            repo_relative("D:/other/thing.rs", root),
            "D:/other/thing.rs"
        );
        // A sibling whose name merely starts the same way is not inside it.
        assert_eq!(
            repo_relative("C:/Users/lyndonswan/Repos/MindLeak-build-2/x.rs", root),
            "C:/Users/lyndonswan/Repos/MindLeak-build-2/x.rs"
        );
        // No declared root: nothing to strip.
        assert_eq!(repo_relative("C:/anywhere/x.rs", None), "C:/anywhere/x.rs");
    }

    #[test]
    fn repo_relative_tolerates_drive_casing_and_a_trailing_slash() {
        assert_eq!(
            repo_relative(
                "c:/users/lyndonswan/Repos/MindLeak-build/src/x.rs",
                Some("C:/Users/lyndonswan/Repos/MindLeak-build/")
            ),
            "src/x.rs"
        );
    }

    #[test]
    fn ignores_vcs_dependency_and_build_output_paths() {
        for junk in [
            ".git/COMMIT_EDITMSG",
            ".git/mine-changelog.patch",
            "target/debug/foo.rs",
            "crates/x/target/tmp.json",
            "editors/vscode/node_modules/pkg/index.js",
            "editors/vscode/coverage/run-3/coverage-0.json",
            "dist/bundle.js",
            ".mindleak/graph.db",
            "crates\\y\\target\\out.json",
        ] {
            assert!(is_ignored_path(junk), "should ignore {junk}");
        }
    }

    #[test]
    fn keeps_real_source_paths() {
        for src in [
            "src/auth.rs",
            "crates/mindleak-core/src/lib.rs",
            "editors/vscode/src/util.ts",
            "scripts/install.mjs",
            "src/target.rs",
        ] {
            assert!(!is_ignored_path(src), "should keep {src}");
        }
    }
}
