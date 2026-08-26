//! Ask git whether a commit id names a real commit in a checkout.

use std::path::Path;

use super::fs::git_command;

/// Whether `sha` names a commit that exists in the checkout at `workspace`.
///
/// `None` means git could not answer — it is absent, or `workspace` is not a
/// repository. A caller must not read that as `Some(false)`: refusing every
/// commit because git is unreachable is a worse failure than the fabrication
/// this exists to catch, so an unanswerable check degrades to the behaviour
/// that existed before it.
///
/// `rev-parse --verify --quiet` rather than the more obvious `cat-file -e`,
/// because only this form's exit code separates the two cases. Measured:
/// `cat-file -e` returns 128 for a fabricated sha AND for running outside a
/// repository, so a guard built on it cannot tell a real refusal from a broken
/// environment — it would either refuse everything or, once that was noticed
/// and softened, silently never fire. `rev-parse --verify` answers 0, 1 and
/// 128 for the three distinct cases.
pub fn commit_exists(workspace: &Path, sha: &str) -> Option<bool> {
    let output = git_command()
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            // `^{commit}` so an object that exists but is a tree or a blob is a
            // "no", not a "yes". Provenance must cite a commit.
            &format!("{sha}^{{commit}}"),
        ])
        .current_dir(workspace)
        .output()
        .ok()?;
    match output.status.code() {
        Some(0) => Some(true),
        Some(1) => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A throwaway repository, so the test does not depend on being run from
    /// inside a checkout. The pre-push hook builds from an isolated snapshot
    /// that is not a git repository at all, where every answer here would
    /// otherwise be `None`.
    struct TempGitRepo {
        path: PathBuf,
    }

    impl TempGitRepo {
        fn create(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "mindleak-commit-exists-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("system clock")
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).expect("create temp repo dir");
            git_run(&path, &["init", "--quiet", "-b", "main"]);
            git_run(&path, &["config", "user.email", "test@example.invalid"]);
            git_run(&path, &["config", "user.name", "MindLeak Test"]);
            std::fs::write(path.join("f.txt"), "x").expect("write fixture file");
            git_run(&path, &["add", "."]);
            git_run(&path, &["commit", "--quiet", "-m", "initial"]);
            Self { path }
        }

        fn rev_parse(&self, rev: &str) -> String {
            let output = git_command()
                .args(["rev-parse", rev])
                .current_dir(&self.path)
                .output()
                .expect("git rev-parse");
            assert!(output.status.success(), "git rev-parse {rev} failed");
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
    }

    impl Drop for TempGitRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn git_run(cwd: &Path, args: &[&str]) {
        let status = git_command()
            .args(args)
            .current_dir(cwd)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed in {cwd:?}");
    }

    #[test]
    fn a_real_commit_resolves() {
        let repo = TempGitRepo::create("real");
        let head = repo.rev_parse("HEAD");
        assert_eq!(commit_exists(&repo.path, &head), Some(true));
    }

    /// The case the shape check cannot reach: forty hex digits, correctly
    /// formed, and naming nothing. This is what an agent composing the tail of
    /// an abbreviation actually produces.
    #[test]
    fn a_well_formed_but_fabricated_sha_does_not_resolve() {
        let repo = TempGitRepo::create("fabricated");
        assert_eq!(
            commit_exists(&repo.path, "0123456789abcdef0123456789abcdef01234567"),
            Some(false)
        );
    }

    /// An object that exists but is not a commit is still a "no": `^{commit}`
    /// is what makes the question the right one.
    #[test]
    fn an_object_that_is_not_a_commit_does_not_resolve() {
        let repo = TempGitRepo::create("tree");
        let tree = repo.rev_parse("HEAD^{tree}");
        assert_eq!(commit_exists(&repo.path, &tree), Some(false));
    }

    /// Unknown, never "no". A path git cannot even be started in must not be
    /// able to refuse a commit that is perfectly real.
    #[test]
    fn an_unusable_workspace_answers_unknown_rather_than_no() {
        let repo = TempGitRepo::create("unusable");
        let head = repo.rev_parse("HEAD");
        let missing = repo.path.join("no-such-directory");
        assert_eq!(commit_exists(&missing, &head), None);
    }

    /// A directory that exists but is not a repository is also "unknown". This
    /// is the case the pre-push hook's isolated build actually hits, and the
    /// one that must never read as a refusal.
    #[test]
    fn a_directory_that_is_not_a_repository_answers_unknown() {
        let repo = TempGitRepo::create("outside");
        let head = repo.rev_parse("HEAD");
        let plain = std::env::temp_dir().join(format!(
            "mindleak-commit-exists-plain-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&plain).expect("create plain dir");
        // `--no-index`-style isolation: a temp dir can still sit inside someone's
        // repository, so make it one git must refuse to look above.
        std::fs::write(plain.join(".git"), "gitdir: nowhere").expect("write decoy gitdir");

        let answer = commit_exists(&plain, &head);

        let _ = std::fs::remove_dir_all(&plain);
        assert_eq!(answer, None);
    }
}
