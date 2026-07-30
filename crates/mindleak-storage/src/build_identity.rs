//! Which build is actually running.
//!
//! Both servers report `<version>+<git-sha>` at MCP `initialize`, and comparing
//! that against the checkout was left to whoever thought to look. Nobody did.
//!
//! A stale local build cost most of a night: `Repos/MindLeak/target/release`
//! held a binary from two days earlier, so every session opened through it
//! resolved a pre-ADR-0054 forked identity, `renew_lease` returned a silent
//! `false`, and the symptom was blamed on the extension four separate times.
//! The binary that was actually wrong was never the one being accused, and
//! nothing on any surface said which build was answering.
//!
//! The *comparison* is only meaningful when the binary was built from the
//! checkout it is serving. An installed release serving an arbitrary repository
//! is expected to differ, so calling that stale would be noise that teaches
//! people to ignore the line that matters.
//!
//! Staying silent about it was the other half of the same mistake. The
//! extension binaries under `~/.vscode/extensions` are the ones the fleet
//! actually runs, and they reported nothing at all — so three servers served a
//! build that predated a merged fix for most of a day, deciding conformance
//! verdicts with it, and no surface said which build was answering. Identity
//! and staleness are different claims: an installed binary cannot be called
//! stale, but it can and must still say what it is.

use std::path::Path;
use std::process::Command;

/// What the running binary has to say about itself at startup.
pub struct BuildNotice {
    /// The line to log.
    pub message: String,
    /// True only when the binary was built from the checkout it serves and is
    /// behind it — the case that warrants a warning. Identity alone does not.
    pub stale: bool,
}

/// What the running binary should report about itself, or `None` when there is
/// nothing meaningful to say.
///
/// `head` and `build_descends_from_head` are passed in rather than read here so
/// the rule stays a pure decision and can be tested without a repository.
/// `build_descends_from_head` is `None` when git could not answer, which is not
/// the same as `Some(false)`: one is ignorance, the other is evidence.
pub fn build_notice(
    executable: &Path,
    workspace: &Path,
    build_sha: &str,
    head: Option<&str>,
    build_descends_from_head: Option<bool>,
) -> Option<BuildNotice> {
    let build_sha = build_sha.trim().to_ascii_lowercase();
    if build_sha.is_empty() || build_sha == "unknown" {
        return None;
    }
    // An installed binary is not built from the workspace it serves, so there is
    // no HEAD it could be behind and nothing here is a staleness claim. It still
    // names itself, because "which build is answering" is the question that went
    // unanswered while the wrong component was blamed.
    if !executable.starts_with(workspace) {
        return Some(BuildNotice {
            message: format!(
                "serving from an installed binary built from {build_sha}; it is not built \
                 from this checkout, so it cannot be compared against HEAD. Check that sha \
                 before blaming behaviour on the workspace."
            ),
            stale: false,
        });
    }
    let head = head?.trim().to_ascii_lowercase();
    if head.is_empty() {
        return None;
    }
    // The build sha is truncated for display, so compare on the shorter one.
    let shared = build_sha.len().min(head.len());
    if build_sha[..shared] == head[..shared] {
        return None;
    }
    // Differing from HEAD is not yet a staleness claim. Stale means the binary
    // was built from something this checkout has since moved past; if the build
    // *descends* from HEAD then the checkout is the thing that is behind, and
    // "rebuild and restart" would replace a newer binary with an older one.
    //
    // This is not hypothetical. The checkout the fleet's servers are compared
    // against sat 599 commits behind main on 2026-07-30, so a binary built from
    // main's tip was reported stale, and following the advice would have
    // reverted an ingest guard merged minutes earlier.
    if build_descends_from_head == Some(true) {
        return Some(BuildNotice {
            message: format!(
                "this checkout is behind the running binary: it was built from {build_sha}, \
                 HEAD is {head}. Rebuilding here would replace the binary with an older \
                 one -- update the checkout instead."
            ),
            stale: false,
        });
    }
    Some(BuildNotice {
        message: format!(
            "running a stale build of this checkout: binary was built from {build_sha}, \
             HEAD is {head}. Rebuild and restart before trusting behaviour or blaming a tool."
        ),
        stale: true,
    })
}

/// Whether `descendant` has `ancestor` in its history, or `None` when git
/// cannot answer -- because it is unavailable, or because either commit is not
/// present in this checkout (a binary built elsewhere).
///
/// `None` is deliberately distinct from `Some(false)`: an unanswerable question
/// must not read as evidence that the build is behind.
pub fn is_ancestor(workspace: &Path, ancestor: &str, descendant: &str) -> Option<bool> {
    let output = Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .current_dir(workspace)
        .output()
        .ok()?;
    match output.status.code() {
        Some(0) => Some(true),
        // 1 is git's answer of "no"; anything else is git failing to answer,
        // most often an unknown revision.
        Some(1) => Some(false),
        _ => None,
    }
}

/// The checkout's current `HEAD`, or `None` when git is unavailable.
pub fn head_sha(workspace: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|sha| !sha.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const WORKSPACE: &str = "/repo";
    const LOCAL: &str = "/repo/target/release/mindleak-mcp";
    const BUILT: &str = "a1b2c3d4e5f6";
    const HEAD: &str = "a1b2c3d4e5f6789000000000000000000000aaaa";

    fn notice(exe: &str, build: &str, head: Option<&str>) -> Option<BuildNotice> {
        // No lineage answer: git could not tell us, which is the conservative
        // case and must keep behaving exactly as it did before.
        notice_with_lineage(exe, build, head, None)
    }

    fn notice_with_lineage(
        exe: &str,
        build: &str,
        head: Option<&str>,
        descends: Option<bool>,
    ) -> Option<BuildNotice> {
        build_notice(
            &PathBuf::from(exe),
            &PathBuf::from(WORKSPACE),
            build,
            head,
            descends,
        )
    }

    /// Regression: a binary built *ahead* of HEAD was reported as stale, and the
    /// advice would have destroyed the deploy.
    ///
    /// What went wrong: the rule compared build sha against HEAD with a plain
    /// string inequality and no ancestry check, so any difference in either
    /// direction produced "running a stale build ... Rebuild and restart".
    ///
    /// Impact, measured on the live fleet 2026-07-30: the checkout the servers
    /// are compared against sat 599 commits behind main, so a binary freshly
    /// built from main's tip was reported stale on every `open_session`. Acting
    /// on that advice rebuilds from the older checkout and replaces the binary
    /// with one 599 commits older -- which would have reverted an ingest guard
    /// merged minutes earlier. A warning whose remedy undoes the fix is worse
    /// than silence, because it is followed.
    ///
    /// The fix: staleness now requires evidence that the build is behind. When
    /// the build descends from HEAD, the checkout is what is behind and the
    /// notice says so instead.
    #[test]
    fn a_build_ahead_of_head_is_not_reported_stale() {
        let notice = notice_with_lineage(LOCAL, "999999999999", Some(HEAD), Some(true))
            .expect("it should still say which build is answering");

        assert!(
            !notice.stale,
            "a build that has HEAD in its history is ahead, not stale: {}",
            notice.message
        );
        assert!(
            !notice.message.contains("Rebuild and restart"),
            "must not advise rebuilding backwards: {}",
            notice.message
        );
        // "Which build is answering" is the question this notice exists for, so
        // both shas must survive the change of verdict.
        assert!(
            notice.message.contains("999999999999"),
            "{}",
            notice.message
        );
        assert!(notice.message.contains(&HEAD[..12]), "{}", notice.message);
    }

    #[test]
    fn a_build_genuinely_behind_head_is_still_stale() {
        // The original regression must keep working: evidence that the build
        // does NOT contain HEAD is exactly the case the warning is for.
        let notice = notice_with_lineage(LOCAL, "999999999999", Some(HEAD), Some(false))
            .expect("should warn");

        assert!(notice.stale, "{}", notice.message);
        assert!(
            notice.message.contains("Rebuild and restart"),
            "{}",
            notice.message
        );
    }

    #[test]
    fn an_unanswerable_lineage_is_not_treated_as_ahead() {
        // `None` is ignorance, not evidence. Silently treating it as "ahead"
        // would trade a false alarm for a missed one, which is the worse half
        // of the trade: the stale build then serves verdicts unannounced.
        let notice =
            notice_with_lineage(LOCAL, "999999999999", Some(HEAD), None).expect("should warn");

        assert!(notice.stale, "{}", notice.message);
    }

    #[test]
    fn a_local_build_behind_head_is_reported() {
        // The regression: a two-day-old local build served every session and
        // nothing said so.
        let notice = notice(LOCAL, "999999999999", Some(HEAD)).expect("should warn");

        assert!(notice.stale, "a local build behind HEAD is the stale case");
        assert!(
            notice.message.contains("999999999999"),
            "{}",
            notice.message
        );
        assert!(notice.message.contains(&HEAD[..12]), "{}", notice.message);
    }

    #[test]
    fn a_local_build_matching_head_is_silent() {
        // The build sha is truncated for display; a prefix match is a match.
        assert!(notice(LOCAL, BUILT, Some(HEAD)).is_none());
    }

    // Regression: the notice was silent for exactly the binaries the fleet runs.
    //
    // What went wrong: an installed binary returned None, on the reasoning that
    // it is not built from the workspace it serves. True — but that only rules
    // out the *staleness* claim, not the identity one. The VS Code extension
    // binaries are built from this repository and deployed by `stage-native`,
    // and they said nothing at all, so three servers ran a build predating a
    // merged fix for most of a day while issuing conformance verdicts with it.
    //
    // The fix: report which build is serving, and never call it stale.
    #[test]
    fn an_installed_binary_names_its_build_without_claiming_staleness() {
        let notice = notice(
            "/home/dev/.vscode/extensions/mindleak/bin/mindleak-mcp",
            "999999999999",
            Some(HEAD),
        )
        .expect("an installed binary must still say which build it is");

        assert!(
            !notice.stale,
            "an installed binary has no HEAD to be behind: {}",
            notice.message
        );
        assert!(
            notice.message.contains("999999999999"),
            "{}",
            notice.message
        );
        assert!(
            !notice.message.to_ascii_lowercase().contains("stale"),
            "identity is not a staleness claim: {}",
            notice.message
        );
    }

    #[test]
    fn an_installed_binary_needs_no_head_to_name_itself() {
        // There is no checkout to compare against, so a missing HEAD must not
        // suppress the identity line.
        let notice = notice(
            "/home/dev/.vscode/extensions/mindleak/bin/mindleak-mcp",
            "999999999999",
            None,
        )
        .expect("identity does not depend on HEAD");

        assert!(!notice.stale);
    }

    #[test]
    fn nothing_to_compare_stays_quiet() {
        assert!(notice(LOCAL, "unknown", Some(HEAD)).is_none());
        assert!(notice(LOCAL, BUILT, None).is_none());
        assert!(notice(LOCAL, "", Some(HEAD)).is_none());
        // A build with no identity has nothing to report wherever it lives.
        assert!(notice("/opt/mindleak/bin/mindleak-mcp", "unknown", None).is_none());
    }
}
