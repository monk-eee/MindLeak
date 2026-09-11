//! Resolve commit identities and their authoritative facts from git.

use std::io;
use std::path::Path;

use serde::Serialize;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommitFacts {
    pub sha: String,
    pub message: String,
    pub changed_files: Vec<String>,
    pub timestamp: i64,
}

/// Read one full commit's own delta, including only authored merge resolutions.
pub fn read_commit(workspace: &Path, sha: &str) -> io::Result<CommitFacts> {
    let sha = sha.trim().to_ascii_lowercase();
    if !matches!(sha.len(), 40 | 64) || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a full commit hash is required",
        ));
    }
    match commit_exists(workspace, &sha) {
        Some(true) => {}
        Some(false) => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "commit does not exist",
            ))
        }
        None => return Err(io::Error::other("Git could not verify the commit")),
    }
    let read = |args: &[&str]| -> io::Result<String> {
        let output = git_command()
            .arg("--no-replace-objects")
            .env("GIT_NO_LAZY_FETCH", "1")
            .args(args)
            .current_dir(workspace)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other("Git could not read the commit facts"));
        }
        if output.stdout.len() > 1_048_576 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "commit facts exceed 1 MiB",
            ));
        }
        String::from_utf8(output.stdout)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    };
    if read(&["rev-parse", "--is-shallow-repository"])?.trim() != "false" {
        return Err(io::Error::other(
            "commit repair requires complete Git history; fetch the missing history before retrying",
        ));
    }
    let metadata = read(&[
        "show",
        "--no-patch",
        "--no-show-signature",
        "--encoding=UTF-8",
        "--format=%H%x00%ct%x00%B",
        &sha,
    ])?;
    let fields: Vec<_> = metadata.splitn(3, '\0').collect();
    let [resolved_sha, timestamp, message] = fields.as_slice() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Git returned incomplete commit facts",
        ));
    };
    if *resolved_sha != sha || message.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Git returned mismatched commit facts",
        ));
    }
    let timestamp = timestamp
        .parse::<i64>()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let paths = read(&[
        "show",
        "--format=",
        "--name-only",
        "-z",
        "--no-renames",
        "--no-ext-diff",
        "--no-textconv",
        "--no-relative",
        "--ignore-submodules=none",
        "--diff-merges=combined",
        &sha,
    ])?;
    let mut changed_files: Vec<_> = paths
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect();
    changed_files.sort();
    changed_files.dedup();
    Ok(CommitFacts {
        sha,
        message: message.trim_end_matches(['\r', '\n']).to_string(),
        changed_files,
        timestamp,
    })
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

    // Branch-wide publication polluted a commit with files it never changed.
    // Repair must read that exact commit's own paths, rationale and timestamp.
    #[test]
    fn commit_facts_are_read_from_the_requested_commit_not_branch_scope() {
        let repo = TempGitRepo::create("facts");
        std::fs::write(repo.path.join("changed file.txt"), "changed\n").unwrap();
        git_run(&repo.path, &["add", "changed file.txt"]);
        let committed = git_command()
            .args([
                "commit",
                "--quiet",
                "-m",
                "fix: exact commit",
                "-m",
                "WHY: attributed to this commit",
            ])
            .env("GIT_COMMITTER_DATE", "2009-02-13T23:31:30Z")
            .current_dir(&repo.path)
            .status()
            .unwrap();
        assert!(committed.success());
        let sha = repo.rev_parse("HEAD");
        std::fs::write(repo.path.join("later.txt"), "later\n").unwrap();
        git_run(&repo.path, &["add", "later.txt"]);
        git_run(&repo.path, &["commit", "--quiet", "-m", "later work"]);

        let facts = read_commit(&repo.path, &sha).unwrap();

        assert_eq!(facts.sha, sha);
        assert_eq!(facts.changed_files, ["changed file.txt"]);
        assert_eq!(facts.timestamp, 1_234_567_890);
        assert_eq!(
            facts.message,
            "fix: exact commit\n\nWHY: attributed to this commit"
        );
    }

    #[test]
    fn commit_facts_refuse_unverifiable_sources_instead_of_returning_an_empty_delta() {
        let repo = TempGitRepo::create("facts-refused");
        assert_eq!(
            read_commit(&repo.path, "HEAD").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            read_commit(&repo.path, &"0".repeat(40)).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            read_commit(&repo.path, &repo.rev_parse("HEAD^{tree}"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        assert!(read_commit(&repo.path.join("absent"), &repo.rev_parse("HEAD")).is_err());
    }

    #[test]
    fn merge_facts_distinguish_integration_from_authored_resolution() {
        let repo = TempGitRepo::create("merge-facts");
        git_run(&repo.path, &["checkout", "--quiet", "-b", "side"]);
        std::fs::write(repo.path.join("f.txt"), "side\n").unwrap();
        git_run(&repo.path, &["add", "f.txt"]);
        git_run(&repo.path, &["commit", "--quiet", "-m", "side"]);
        git_run(&repo.path, &["checkout", "--quiet", "main"]);
        std::fs::write(repo.path.join("main.txt"), "main\n").unwrap();
        git_run(&repo.path, &["add", "main.txt"]);
        git_run(&repo.path, &["commit", "--quiet", "-m", "main"]);
        git_run(&repo.path, &["merge", "--quiet", "--no-edit", "side"]);
        let clean = read_commit(&repo.path, &repo.rev_parse("HEAD")).unwrap();
        assert!(
            clean.changed_files.is_empty(),
            "a clean merge authored no files"
        );

        git_run(
            &repo.path,
            &["checkout", "--quiet", "-b", "conflicting", "HEAD^1"],
        );
        std::fs::write(repo.path.join("f.txt"), "conflicting\n").unwrap();
        git_run(&repo.path, &["add", "f.txt"]);
        git_run(&repo.path, &["commit", "--quiet", "-m", "conflicting"]);
        git_run(&repo.path, &["checkout", "--quiet", "main"]);
        let conflict = git_command()
            .args(["merge", "--no-edit", "conflicting"])
            .current_dir(&repo.path)
            .output()
            .unwrap();
        assert_eq!(conflict.status.code(), Some(1));
        std::fs::write(repo.path.join("f.txt"), "resolved\n").unwrap();
        git_run(&repo.path, &["add", "f.txt"]);
        git_run(&repo.path, &["commit", "--quiet", "-m", "resolved"]);
        let resolved = read_commit(&repo.path, &repo.rev_parse("HEAD")).unwrap();
        assert_eq!(resolved.changed_files, ["f.txt"]);
    }

    #[test]
    fn commit_facts_ignore_replacement_refs_that_rewrite_the_named_object() {
        let repo = TempGitRepo::create("replacement-facts");
        let original = repo.rev_parse("HEAD");
        std::fs::write(repo.path.join("replacement.txt"), "replacement\n").unwrap();
        git_run(&repo.path, &["add", "replacement.txt"]);
        git_run(&repo.path, &["commit", "--quiet", "-m", "replacement"]);
        let replacement = repo.rev_parse("HEAD");
        git_run(&repo.path, &["replace", &original, &replacement]);

        let facts = read_commit(&repo.path, &original).unwrap();
        assert_eq!(facts.sha, original);
        assert_eq!(facts.message, "initial");
        assert_eq!(facts.changed_files, ["f.txt"]);
    }

    // A shallow boundary looks like a root commit to Git and reports the whole
    // tree as changed. It must not authorize a supposedly verified correction.
    #[test]
    fn commit_facts_refuse_a_shallow_history_instead_of_claiming_the_whole_tree() {
        let repo = TempGitRepo::create("shallow-facts");
        std::fs::write(repo.path.join("new.txt"), "new\n").unwrap();
        git_run(&repo.path, &["add", "new.txt"]);
        git_run(&repo.path, &["commit", "--quiet", "-m", "one file"]);
        git_run(
            &repo.path,
            &[
                "clone",
                "--quiet",
                "--no-local",
                "--depth",
                "1",
                repo.path.to_str().unwrap(),
                "shallow",
            ],
        );
        let shallow = repo.path.join("shallow");
        let error = read_commit(&shallow, &repo.rev_parse("HEAD")).unwrap_err();
        assert!(error.to_string().contains("complete Git history"));
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
