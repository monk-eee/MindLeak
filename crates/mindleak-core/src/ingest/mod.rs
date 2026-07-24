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
