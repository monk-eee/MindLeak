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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    struct TempGitRepo {
        path: PathBuf,
    }

    impl TempGitRepo {
        fn create(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "mindleak-worktree-roots-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("system clock")
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).expect("create temp repo dir");
            git_run(&path, &["init", "--quiet", "-b", "main"]);
            Self { path }
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
    fn a_repository_with_no_linked_worktrees_lists_only_its_own_root() {
        let repo = TempGitRepo::create("single");

        let roots = worktree_roots(&repo.path);

        assert_eq!(roots.len(), 1, "{roots:?}");
        // Compare canonicalized paths rather than raw strings: git reports a
        // resolved path that can differ in case/separators/symlink form from
        // whatever the OS temp dir handed us.
        let reported = Path::new(&roots[0])
            .canonicalize()
            .expect("canonicalize reported root");
        let expected = repo
            .path
            .canonicalize()
            .expect("canonicalize temp repo path");
        assert_eq!(reported, expected);
    }

    #[test]
    fn a_linked_worktree_lists_both_roots() {
        let repo = TempGitRepo::create("main");
        git_run(
            &repo.path,
            &["config", "user.email", "test@example.invalid"],
        );
        git_run(&repo.path, &["config", "user.name", "MindLeak Test"]);
        std::fs::write(repo.path.join("f.txt"), "x").expect("write fixture file");
        git_run(&repo.path, &["add", "."]);
        git_run(&repo.path, &["commit", "-m", "initial"]);
        let linked = repo.path.with_file_name(format!(
            "{}-linked",
            repo.path
                .file_name()
                .expect("repo dir name")
                .to_string_lossy()
        ));
        git_run(
            &repo.path,
            &[
                "worktree",
                "add",
                "-b",
                "feature",
                linked.to_str().expect("utf8 path"),
            ],
        );

        let roots = worktree_roots(&repo.path);

        assert_eq!(roots.len(), 2, "{roots:?}");
        let canonical: Vec<PathBuf> = roots
            .iter()
            .map(|root| {
                Path::new(root)
                    .canonicalize()
                    .expect("canonicalize reported root")
            })
            .collect();
        assert!(canonical.contains(&repo.path.canonicalize().expect("canonicalize repo path")));
        assert!(canonical.contains(&linked.canonicalize().expect("canonicalize linked path")));

        let _ = std::fs::remove_dir_all(&linked);
    }

    #[test]
    fn a_directory_git_cannot_answer_for_returns_empty() {
        let dir = std::env::temp_dir().join(format!(
            "mindleak-worktree-roots-not-a-repo-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create plain dir");

        let roots = worktree_roots(&dir);

        assert!(roots.is_empty(), "{roots:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
