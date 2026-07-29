//! Verify that a commit on the integration branch is the merge of a task's work.
//!
//! ADR-0058: a merge is evidence. It is stronger evidence than the bundle it
//! replaces — a merge to a protected branch has passed review and CI, which is
//! more than an ingest call can attest — but only if "does this commit
//! correspond to this task" stays a deterministic question. The moment it
//! becomes a judgement call, this stops proving work and starts laundering it.
//!
//! So every check here is something git can answer with a yes or a no, and
//! nothing here reads a commit message, asks a model, or trusts the caller's
//! description of what was done.

use std::path::Path;
use std::process::Command;

use crate::error::{LodestarError, Result};

/// What a verified merge established, in the caller's words as little as possible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeProof {
    /// The full commit hash, as git resolved it.
    pub commit: String,
    /// Repository-relative paths the commit touched.
    pub changed_paths: Vec<String>,
}

/// Run one git command in `root`, returning stdout on success.
///
/// Every inherited git environment variable is cleared. A child git that
/// inherits `GIT_DIR` or `GIT_INDEX_FILE` from the process that spawned it
/// operates on *that* repository instead of this one, which turns a read into a
/// write against somebody else's checkout. This has already happened once in
/// this repository (PR #21), and a verification step that can corrupt the thing
/// it verifies is worse than no verification at all.
fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .output()
        .map_err(|error| LodestarError::Invalid(format!("git could not be run: {error}")))?;
    if !output.status.success() {
        return Err(LodestarError::Invalid(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Verify that `commit` exists, is reachable from `integration`, and touched
/// something inside `scope`.
///
/// `scope` is the task's declared path scope (ADR-0024). It is advisory when an
/// agent declares it, but here it is the only thing tying a commit to a task, so
/// an empty scope means the caller has to be told rather than waved through: a
/// task that declared nothing would otherwise accept any merged commit in the
/// repository as its own receipt.
pub fn verify_merge(
    root: &Path,
    commit: &str,
    integration: &str,
    scope: &[String],
) -> Result<MergeProof> {
    let commit = commit.trim();
    if commit.is_empty() {
        return Err(LodestarError::Invalid(
            "no merge commit given; name the commit on the integration branch that carried this work"
                .to_string(),
        ));
    }

    // Resolve rather than trust. A caller-supplied string that merely looks like
    // a hash proves nothing, and `^{commit}` refuses a tag or a tree that would
    // otherwise resolve to something that is not a commit at all.
    let resolved = git(
        root,
        &["rev-parse", "--verify", &format!("{commit}^{{commit}}")],
    )
    .map_err(|_| {
        LodestarError::Invalid(format!(
            "git cannot resolve {commit} in this repository; \
                 pass the full hash of the merge commit as `git rev-parse HEAD` reports it"
        ))
    })?;

    // Reachable from the integration branch is the whole point: an unmerged
    // commit on a side branch is work in progress, not work that shipped.
    let reachable = Command::new("git")
        .args(["merge-base", "--is-ancestor", &resolved, integration])
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .status()
        .map_err(|error| LodestarError::Invalid(format!("git could not be run: {error}")))?;
    if !reachable.success() {
        return Err(LodestarError::Invalid(format!(
            "{resolved} is not reachable from {integration}; \
             work that has not landed cannot be evidence that it shipped"
        )));
    }

    let listing = git(
        root,
        &["show", "--pretty=format:", "--name-only", &resolved],
    )?;
    let changed_paths: Vec<String> = listing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();

    if scope.is_empty() {
        return Err(LodestarError::Invalid(format!(
            "task declared no path scope, so nothing ties {resolved} to it; \
             declare paths when claiming, or complete with an evidence bundle instead"
        )));
    }
    if !changed_paths.iter().any(|path| {
        scope
            .iter()
            .any(|declared| crate::scope::covers(declared, path))
    }) {
        return Err(LodestarError::Invalid(format!(
            "{resolved} touches nothing inside this task's declared scope; \
             a merge that changed none of the task's paths is somebody else's work"
        )));
    }

    Ok(MergeProof {
        commit: resolved,
        changed_paths,
    })
}
