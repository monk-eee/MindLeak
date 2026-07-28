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
//! The comparison is only meaningful when the binary was built *from the
//! checkout it is serving*. An installed release serving an arbitrary
//! repository is expected to differ, and warning about that would be noise that
//! teaches people to ignore the line that matters.

use std::path::Path;
use std::process::Command;

/// The notice to log when the running binary is a stale build of this checkout,
/// or `None` when there is nothing meaningful to say.
///
/// `head` is passed in rather than read here so the rule stays a pure decision
/// and can be tested without a repository.
pub fn stale_build_notice(
    executable: &Path,
    workspace: &Path,
    build_sha: &str,
    head: Option<&str>,
) -> Option<String> {
    // An installed binary is not built from the workspace it serves, so a
    // difference carries no information.
    if !executable.starts_with(workspace) {
        return None;
    }
    let build_sha = build_sha.trim().to_ascii_lowercase();
    let head = head?.trim().to_ascii_lowercase();
    if build_sha.is_empty() || build_sha == "unknown" || head.is_empty() {
        return None;
    }
    // The build sha is truncated for display, so compare on the shorter one.
    let shared = build_sha.len().min(head.len());
    if build_sha[..shared] == head[..shared] {
        return None;
    }
    Some(format!(
        "running a stale build of this checkout: binary was built from {build_sha}, \
         HEAD is {head}. Rebuild and restart before trusting behaviour or blaming a tool."
    ))
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

    fn notice(exe: &str, build: &str, head: Option<&str>) -> Option<String> {
        stale_build_notice(&PathBuf::from(exe), &PathBuf::from(WORKSPACE), build, head)
    }

    #[test]
    fn a_local_build_behind_head_is_reported() {
        // The regression: a two-day-old local build served every session and
        // nothing said so.
        let message = notice(LOCAL, "999999999999", Some(HEAD)).expect("should warn");

        assert!(message.contains("999999999999"), "{message}");
        assert!(message.contains(&HEAD[..12]), "{message}");
    }

    #[test]
    fn a_local_build_matching_head_is_silent() {
        // The build sha is truncated for display; a prefix match is a match.
        assert_eq!(notice(LOCAL, BUILT, Some(HEAD)), None);
    }

    #[test]
    fn an_installed_binary_is_never_compared() {
        // An extension release serving an arbitrary repository is expected to
        // differ. Warning here would be noise that buries the real case.
        assert_eq!(
            notice(
                "/home/dev/.vscode/extensions/mindleak/bin/mindleak-mcp",
                "999999999999",
                Some(HEAD)
            ),
            None
        );
    }

    #[test]
    fn nothing_to_compare_stays_quiet() {
        assert_eq!(notice(LOCAL, "unknown", Some(HEAD)), None);
        assert_eq!(notice(LOCAL, BUILT, None), None);
        assert_eq!(notice(LOCAL, "", Some(HEAD)), None);
    }
}
