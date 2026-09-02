use std::collections::HashSet;

use crate::graph::STRUCTURE_EXTRACTOR_VERSION;
use crate::{ingest, ForgetOutcome, MindLeak, ReconcileOutcome, Result};

impl MindLeak {
    /// Forget a deleted file: reap its structure (the symbols it defined and the
    /// artifact node) and every edge touching them. Called when the workspace
    /// reports a file removed or renamed, so the graph stops carrying structure
    /// for a path that no longer exists instead of waiting ~a month for it to
    /// decay. A no-op when the path was never ingested.
    pub fn forget_file(&self, path: &str) -> Result<ForgetOutcome> {
        let norm = self.repo_relative(path);
        let artifact_id = format!("artifact:{norm}");
        self.store.forget_artifact(&artifact_id)
    }

    /// Reconcile the graph against the workspace's current file set: forget every
    /// file artifact whose path is not in `current_paths` (deleted or moved
    /// outside the editor's delete events) or is build/VCS junk. This cleans
    /// stale structure in one pass rather than waiting ~a month for it to decay,
    /// and catches deletions the editor's `forget_file` hook cannot see (e.g. a
    /// terminal `git rm` or an external move).
    pub fn reconcile_workspace(&self, current_paths: &[String]) -> Result<ReconcileOutcome> {
        let keep = self.workspace_path_set(current_paths);
        let mut outcome = ReconcileOutcome::default();
        for artifact_id in self.store.artifact_ids()? {
            let path = artifact_id
                .strip_prefix("artifact:")
                .unwrap_or(&artifact_id);
            if ingest::is_ignored_path(path) || !keep.contains(path) {
                let forgotten = self.store.forget_artifact(&artifact_id)?;
                if forgotten.nodes_removed > 0 || forgotten.edges_removed > 0 {
                    outcome.files_forgotten += 1;
                    outcome.nodes_removed += forgotten.nodes_removed;
                    outcome.edges_removed += forgotten.edges_removed;
                }
            }
        }
        let status = self.structure_status_for(&keep)?;
        outcome.extractor_version = status.extractor_version;
        outcome.stale_paths = status.stale_paths;
        Ok(outcome)
    }

    /// Read which tracked artifacts need structural refresh without forgetting
    /// anything. Used by re-ingest dry runs so inspection remains mutation-free.
    pub fn workspace_structure_status(&self, current_paths: &[String]) -> Result<ReconcileOutcome> {
        self.structure_status_for(&self.workspace_path_set(current_paths))
    }

    fn workspace_path_set(&self, current_paths: &[String]) -> HashSet<String> {
        current_paths
            .iter()
            .map(|path| self.repo_relative(path))
            .collect()
    }

    fn structure_status_for(&self, keep: &HashSet<String>) -> Result<ReconcileOutcome> {
        let known_paths: HashSet<String> = self
            .store
            .artifact_ids()?
            .into_iter()
            .filter_map(|id| id.strip_prefix("artifact:").map(str::to_string))
            .collect();
        let stale_paths = self
            .store
            .stale_artifact_ids(STRUCTURE_EXTRACTOR_VERSION)?
            .into_iter()
            .filter_map(|id| id.strip_prefix("artifact:").map(str::to_string))
            .filter(|path| keep.contains(path) && !ingest::is_ignored_path(path))
            .collect();
        let mut missing_paths: Vec<String> = keep
            .difference(&known_paths)
            .filter(|path| !ingest::is_ignored_path(path))
            .cloned()
            .collect();
        missing_paths.sort();
        Ok(ReconcileOutcome {
            extractor_version: STRUCTURE_EXTRACTOR_VERSION,
            stale_paths,
            missing_paths,
            ..ReconcileOutcome::default()
        })
    }
}
