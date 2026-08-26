//! Zero-token deterministic ingestion: turn raw telemetry into graph triples
//! using pattern matching only (no LLM tokens on the write path).

pub mod ast;
pub mod execution;
pub mod git;
pub(crate) mod javascript;
pub mod manifest;
pub(crate) mod source_mask;
pub mod structure;
pub mod tool_invocation;

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

/// Normalise a path *and* make it relative to whichever `root` contains it.
///
/// Node ids are repo-relative by contract. Editor sensors report absolute
/// paths, and every worktree of a repository shares one graph (ADR-0038), so an
/// absolute id splits a single file across as many identities as there are
/// checkouts -- fragmenting its history, its reinforcement, and its governance.
///
/// All of a repository's worktree roots are candidates, not just the server's
/// own. A file saved in a sibling checkout is the same file, and refusing it
/// loses it entirely: measured 2026-07-30, 203 of 291 ingest calls were refused
/// for exactly that reason. Rooting each window at its own worktree (ADR-0073)
/// is the cheaper fix, but it depends on an operational habit; this makes the
/// answer the same whichever window did the saving.
///
/// The longest matching root wins, and a root only matches on a path boundary,
/// so `.../MindLeak` never swallows a path under `.../MindLeak-build`.
///
/// A path outside every root, or a call with no roots at all, is returned
/// normalised but otherwise untouched: guessing a relative form for something
/// that is genuinely elsewhere would invent a file that does not exist.
pub(crate) fn repo_relative(path: &str, roots: &[&str]) -> String {
    let normalized = normalize_path(path);
    // Windows paths are case-insensitive and arrive with either drive casing,
    // so compare case-insensitively while keeping the caller's own spelling.
    let candidate = normalized.to_ascii_lowercase();
    let mut matched = None;
    for root in roots {
        let root = normalize_path(root);
        let root = root.trim_end_matches('/');
        if root.is_empty() {
            continue;
        }
        let prefix = format!("{}/", root.to_ascii_lowercase());
        if candidate.starts_with(&prefix) && matched.is_none_or(|best| prefix.len() > best) {
            matched = Some(prefix.len());
        }
    }
    match matched {
        Some(len) => normalized[len..].to_string(),
        None => normalized,
    }
}

/// True when a normalised path still names an absolute location: a POSIX or
/// UNC root (`/usr/...`, `//host/share`) or a Windows drive (`c:/...`).
///
/// `repo_relative` deliberately returns a path it cannot place untouched, which
/// is right for a helper but wrong for a node id. A path that survives
/// `repo_relative` still absolute belongs to *another* checkout of this
/// repository, and minting an id from it is what splits one file across as many
/// identities as there are worktrees. Callers on the write path use this to
/// refuse that id rather than create the duplicate the repair pass then has to
/// find and merge.
pub(crate) fn is_absolute_path(path: &str) -> bool {
    let path = normalize_path(path);
    if path.starts_with('/') {
        return true;
    }
    // `c:/x`, `c:x` and a bare `c:` are all drive-relative or drive-absolute;
    // none of them is a repo-relative path.
    let mut chars = path.chars();
    matches!((chars.next(), chars.next()), (Some(drive), Some(':')) if drive.is_ascii_alphabetic())
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
        let build = ["C:/Users/lyndonswan/Repos/MindLeak-build"];
        let gap = ["C:/Users/lyndonswan/Repos/MindLeak-gap"];

        assert_eq!(
            repo_relative("C:/Users/lyndonswan/Repos/MindLeak-build/AGENTS.md", &build),
            "AGENTS.md"
        );
        // Two different checkouts must resolve to the same node id.
        assert_eq!(
            repo_relative(
                "C:\\Users\\lyndonswan\\Repos\\MindLeak-gap\\AGENTS.md",
                &gap
            ),
            repo_relative("C:/Users/lyndonswan/Repos/MindLeak-build/AGENTS.md", &build)
        );
    }

    /// Regression: a file saved in a sibling worktree was refused, so it never
    /// reached the graph at all.
    ///
    /// What went wrong: only the server's own workspace root was a candidate, so
    /// a path under any other worktree of the SAME repository stayed absolute
    /// and the ingest guard then refused it.
    ///
    /// Impact, measured 2026-07-30: 203 of 291 `ingest_file` calls were refused
    /// (69.8%), naming paths like
    /// `c:/Users/lyndonswan/Repos/MindLeak-rustimports/scripts/silent-knowledge.mjs`.
    /// Every worktree of a repository shares one graph (ADR-0038), so those were
    /// files this graph should hold, dropped on the floor. Rooting each window
    /// at its own worktree (ADR-0073) fixes it at the source, but that depends on
    /// an operational habit; this makes the answer the same either way.
    #[test]
    fn a_path_from_a_sibling_worktree_resolves_to_the_same_id() {
        let roots = [
            "C:/Users/lyndonswan/Repos/MindLeak",
            "C:/Users/lyndonswan/Repos/MindLeak-rustimports",
            "C:/Users/lyndonswan/Repos/MindLeak-build",
        ];

        // The server is rooted at the primary checkout; the file was saved in a
        // sibling. It is still this repository's file.
        assert_eq!(
            repo_relative(
                "c:/Users/lyndonswan/Repos/MindLeak-rustimports/scripts/silent-knowledge.mjs",
                &roots
            ),
            "scripts/silent-knowledge.mjs"
        );

        // Every worktree agrees on the id, which is the whole point.
        assert_eq!(
            repo_relative("C:/Users/lyndonswan/Repos/MindLeak-build/AGENTS.md", &roots),
            repo_relative("C:/Users/lyndonswan/Repos/MindLeak/AGENTS.md", &roots)
        );

        // The longest match wins on a path boundary, so the primary checkout
        // never swallows a sibling whose name merely extends it. Getting this
        // wrong would silently produce `-rustimports/scripts/...` as an id.
        assert_eq!(
            repo_relative("C:/Users/lyndonswan/Repos/MindLeak-build/x.rs", &roots),
            "x.rs"
        );

        // A path under no root of this repository is still not placeable.
        assert_eq!(
            repo_relative("C:/Users/lyndonswan/Repos/SomethingElse/x.rs", &roots),
            "C:/Users/lyndonswan/Repos/SomethingElse/x.rs"
        );
    }

    #[test]
    fn repo_relative_leaves_alone_what_it_cannot_place() {
        let root = ["C:/Users/lyndonswan/Repos/MindLeak-build"];

        // Already relative: untouched apart from separators.
        assert_eq!(repo_relative("crates\\x\\lib.rs", &root), "crates/x/lib.rs");
        // Genuinely outside the checkout: inventing a relative form would name a
        // file that does not exist.
        assert_eq!(
            repo_relative("D:/other/thing.rs", &root),
            "D:/other/thing.rs"
        );
        // A sibling whose name merely starts the same way is not inside it.
        assert_eq!(
            repo_relative("C:/Users/lyndonswan/Repos/MindLeak-build-2/x.rs", &root),
            "C:/Users/lyndonswan/Repos/MindLeak-build-2/x.rs"
        );
        // No declared root: nothing to strip.
        assert_eq!(repo_relative("C:/anywhere/x.rs", &[]), "C:/anywhere/x.rs");
    }

    #[test]
    fn absolute_paths_are_recognised_in_every_spelling_a_sensor_emits() {
        for absolute in [
            "c:/Users/agent/Repos/MindLeak/src/x.rs",
            "C:\\Users\\agent\\Repos\\MindLeak\\src\\x.rs",
            "/home/agent/checkout/src/x.rs",
            "//fileserver/share/src/x.rs",
            "d:relative-to-drive-cwd.rs",
        ] {
            assert!(is_absolute_path(absolute), "should be absolute: {absolute}");
        }

        // Repo-relative spellings are what node ids are made of, including the
        // traversal forms, which are relative even though they leave the folder.
        for relative in [
            "crates/mindleak-core/src/lib.rs",
            "src\\x.rs",
            "./x.rs",
            "../sibling/x.rs",
            "x.rs",
        ] {
            assert!(
                !is_absolute_path(relative),
                "should be relative: {relative}"
            );
        }
    }

    #[test]
    fn repo_relative_tolerates_drive_casing_and_a_trailing_slash() {
        assert_eq!(
            repo_relative(
                "c:/users/lyndonswan/Repos/MindLeak-build/src/x.rs",
                &["C:/Users/lyndonswan/Repos/MindLeak-build/"]
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
