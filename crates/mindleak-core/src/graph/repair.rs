//! Collapse duplicate node identities onto one id.
//!
//! Node ids are repo-relative by contract, but absolute paths reached the graph
//! for as long as nothing made them relative. Every worktree of a repository
//! shares one graph (ADR-0038), so a single file could hold a different identity
//! in each checkout — measured here as 871 absolute ids across 7 worktrees, with
//! 590 files living under two identities at once.
//!
//! Splitting a file's identity splits everything derived from it: reinforcement
//! (so corroborated signal decays like a one-off, ADR-0005), `check_overlap`
//! (two agents on one file never collide), governance (a binding covers only one
//! spelling), and recall (the same file returned twice).
//!
//! Repair is idempotent and prefix-scoped, so each server heals the ids under
//! its own checkout and a second pass finds nothing left to do.

use rusqlite::params;

use crate::graph::GraphStore;
use crate::ingest::repo_relative;
use crate::Result;

pub use crate::graph::types::RepairOutcome;

mod collapse;

#[cfg(test)]
mod tests;

impl GraphStore {
    /// Rewrite every node id that spells its path absolutely under `root` to the
    /// repo-relative id the rest of the fleet writes, merging into the relative
    /// node when one already exists.
    ///
    /// Then collapse absolute ids spelled under *any other* checkout of this
    /// repository, which the prefix pass cannot see. Being prefix-scoped assumes
    /// every worktree eventually hosts a server that heals its own ids, and that
    /// is not true: a worktree an agent works in without ever starting a server
    /// there leaves its ids orphaned permanently. Measured 2026-07-29, 43 of 247
    /// tracked files could not be re-ingested at all because a sibling
    /// checkout's absolute id still owned their structural edges.
    pub fn repair_workspace_paths(&self, root: &str) -> Result<RepairOutcome> {
        let root = root.trim_end_matches(['/', '\\']);
        if root.is_empty() {
            return Ok(RepairOutcome::default());
        }

        let duplicates = self.absolute_path_nodes(root)?;
        let mut outcome = RepairOutcome::default();
        for (from, to) in duplicates {
            if from == to {
                continue;
            }
            let existed = self.merge_node(&from, &to)?;
            outcome.nodes_rewritten += 1;
            if existed {
                outcome.nodes_merged += 1;
            }
        }
        self.collapse_known_duplicates(&mut outcome)?;
        Ok(outcome)
    }

    /// Node ids under `root`, paired with the repo-relative id they should carry.
    fn absolute_path_nodes(&self, root: &str) -> Result<Vec<(String, String)>> {
        let mut statement = self
            .conn
            .prepare("SELECT id FROM nodes WHERE id LIKE 'artifact:%' OR id LIKE 'symbol:%'")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;

        let mut out = Vec::new();
        for row in rows {
            let id = row?;
            let Some((prefix, rest)) = id.split_once(':') else {
                continue;
            };
            // A symbol id is `symbol:<path>:<name>`; only the path is rewritten.
            let (path, suffix) = match prefix {
                "symbol" => match rest.rsplit_once(':') {
                    Some((path, name)) => (path.to_string(), format!(":{name}")),
                    None => continue,
                },
                _ => (rest.to_string(), String::new()),
            };
            let relative = repo_relative(&path, &[root]);
            if relative != path {
                out.push((id.clone(), format!("{prefix}:{relative}{suffix}")));
            }
        }
        Ok(out)
    }

    /// Move `from`'s edges and node row onto `to`, then delete `from`.
    ///
    /// Returns whether `to` already existed, which is the difference between a
    /// rename and a genuine merge.
    ///
    /// Edges are moved *before* the node is deleted: the schema cascades edge
    /// deletion from nodes, so dropping the node first would take the history
    /// this repair exists to preserve.
    fn merge_node(&self, from: &str, to: &str) -> Result<bool> {
        let existed: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE id = ?1)",
            params![to],
            |row| row.get(0),
        )?;

        // Carry the node across first so the edges below have an endpoint.
        // The relative label wins; the earliest sighting and latest access
        // survive, because those are facts about the file, not the spelling.
        self.conn.execute(
            "INSERT INTO nodes (id, type, label, content, created_at, last_accessed_at)
             SELECT ?2, type, ?3, content, created_at, last_accessed_at
             FROM nodes WHERE id = ?1
             ON CONFLICT(id) DO UPDATE SET
                 created_at = MIN(nodes.created_at, excluded.created_at),
                 last_accessed_at = MAX(nodes.last_accessed_at, excluded.last_accessed_at),
                 content = COALESCE(nodes.content, excluded.content)",
            params![from, to, label_for(to)],
        )?;

        // Replay reinforcement rather than pick a winner: had these been one
        // edge all along it would have taken both halves' reinforcements, and
        // `weight + 0.05` per reinforcement is exactly the write-path rule.
        for (column, other) in [("source_id", "target_id"), ("target_id", "source_id")] {
            let sql = format!(
                "INSERT INTO edges (
                     source_id, target_id, relation, weight, half_life_hours,
                     updated_at, first_seen, reinforcement_count, owner_id
                 )
                 SELECT
                     {source}, {target}, relation, weight, half_life_hours,
                     updated_at, first_seen, reinforcement_count, owner_id
                 FROM edges WHERE {column} = ?1
                 ON CONFLICT(source_id, target_id, relation) DO UPDATE SET
                     weight = MIN(1.0, edges.weight + 0.05 * excluded.reinforcement_count),
                     reinforcement_count =
                         edges.reinforcement_count + excluded.reinforcement_count,
                     updated_at = MAX(edges.updated_at, excluded.updated_at),
                     first_seen = MIN(edges.first_seen, excluded.first_seen),
                     half_life_hours = CASE
                         WHEN excluded.updated_at > edges.updated_at
                         THEN excluded.half_life_hours ELSE edges.half_life_hours END,
                     owner_id = COALESCE(edges.owner_id, excluded.owner_id)",
                source = if column == "source_id" { "?2" } else { other },
                target = if column == "source_id" { other } else { "?2" },
                column = column,
            );
            self.conn.execute(&sql, params![from, to])?;
        }

        // Ownership follows the identity, not just the endpoints. `owner_id`
        // records which artifact owns a structural snapshot (ADR-0007), and
        // moving an edge without it leaves the snapshot owned by an id that is
        // about to be deleted. `replace_structure` then refuses every later
        // ingest of that file — "structural edge is owned by <old id>, not
        // <new id>" — so the file can never be re-extracted, which is exactly
        // the state 43 files were found in. The endpoints had been rewritten
        // for as long as this merge has existed; the ownership never was.
        self.conn.execute(
            "UPDATE edges SET owner_id = ?2 WHERE owner_id = ?1",
            params![from, to],
        )?;

        // Cascades the stale edges and the stale embedding; the index pass will
        // re-embed the survivor under its one true id.
        self.conn
            .execute("DELETE FROM nodes WHERE id = ?1", params![from])?;
        Ok(existed)
    }
}

/// The human label for a node id: the path, without the `artifact:`/`symbol:`
/// prefix, matching what ingestion writes.
fn label_for(id: &str) -> String {
    id.split_once(':')
        .map(|(_, rest)| rest.to_string())
        .unwrap_or_else(|| id.to_string())
}
