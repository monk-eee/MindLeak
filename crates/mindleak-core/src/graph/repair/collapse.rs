//! Collapse an absolute id onto the repo-relative twin the graph already holds.
//!
//! The prefix pass in the parent module heals ids under *this* checkout. It
//! assumes every worktree eventually hosts a server that heals its own, and
//! that is not true: a worktree an agent works in without ever starting a
//! server there leaves its ids orphaned permanently. Every worktree of a
//! repository shares one graph (ADR-0038), so those ids name files this
//! repository already knows — they are duplicates, not strangers.
//!
//! The warrant here is evidence rather than a guess about where a checkout
//! begins: a merge target must be a repo-relative id the graph *already holds*.
//! An absolute path with no such twin is left exactly alone, because inventing
//! a relative form for something genuinely elsewhere would invent a file that
//! does not exist — the rule the prefix pass protects, unchanged.

use rusqlite::params;

use crate::graph::types::RepairOutcome;
use crate::graph::GraphStore;
use crate::Result;

/// True when a path names a filesystem root rather than a repo-relative file:
/// a POSIX `/at/root` or a Windows `C:/at/root`. Ids are repo-relative by
/// contract, so anything matching this is a duplicate identity waiting to be
/// collapsed.
pub(super) fn is_absolute_path(path: &str) -> bool {
    if path.starts_with('/') {
        return true;
    }
    let mut chars = path.chars();
    matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(drive), Some(':'), Some('/')) if drive.is_ascii_alphabetic()
    )
}

impl GraphStore {
    /// Merge every absolute id whose repo-relative twin is already in the graph,
    /// whichever checkout spelled it, then reclaim ownership left behind.
    ///
    /// Deliberately independent of any declared root. The prefix pass is a no-op
    /// without one, and a server that never declares a workspace is exactly the
    /// case that leaves a sibling checkout's ids orphaned forever.
    pub fn collapse_known_duplicates(&self, outcome: &mut RepairOutcome) -> Result<()> {
        for (from, path, prefix, suffix) in self.absolute_ids()? {
            let Some(relative) = self.longest_known_suffix(&path, &prefix, &suffix)? else {
                continue;
            };
            let to = format!("{prefix}:{relative}{suffix}");
            if to == from {
                continue;
            }
            let existed = self.merge_node(&from, &to)?;
            outcome.nodes_rewritten += 1;
            if existed {
                outcome.nodes_merged += 1;
            }
        }
        self.reclaim_absolute_ownership()?;
        Ok(())
    }

    /// Every artifact/symbol id whose path is spelled absolutely, split into the
    /// parts an id is rebuilt from.
    fn absolute_ids(&self) -> Result<Vec<(String, String, String, String)>> {
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
            // A symbol id is `symbol:<path>:<name>`; only the path is a duplicate.
            let (path, suffix) = match prefix {
                "symbol" => match rest.rsplit_once(':') {
                    Some((path, name)) => (path.to_string(), format!(":{name}")),
                    None => continue,
                },
                _ => (rest.to_string(), String::new()),
            };
            if is_absolute_path(&path) {
                out.push((id.clone(), path, prefix.to_string(), suffix));
            }
        }
        Ok(out)
    }

    /// The longest proper suffix of `path` that the graph already holds under
    /// the same id shape. Longest wins so a full relative path always beats a
    /// bare filename that happens to collide.
    fn longest_known_suffix(
        &self,
        path: &str,
        prefix: &str,
        suffix: &str,
    ) -> Result<Option<String>> {
        let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
        // Start one segment in: the whole path is the absolute id being retired,
        // so it can never be its own twin.
        for start in 1..segments.len() {
            let candidate = segments[start..].join("/");
            if candidate.is_empty() {
                continue;
            }
            let id = format!("{prefix}:{candidate}{suffix}");
            if self.node_exists(&id)? {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    /// Re-point structural ownership that still names an absolute id.
    ///
    /// `owner_id` is not an endpoint, so it survives the node it names being
    /// deleted, and an owner naming a node that no longer exists is worse than a
    /// duplicate: `replace_structure` refuses every later ingest of that file
    /// with "structural edge is owned by <absolute id>, not <relative id>", and
    /// with the absolute node gone there is nothing left for a node-level repair
    /// to find. The file becomes permanently un-re-extractable, quietly.
    ///
    /// Keyed off ownership rather than nodes precisely so it heals that orphaned
    /// state as well as the live one.
    fn reclaim_absolute_ownership(&self) -> Result<()> {
        let mut statement = self
            .conn
            .prepare("SELECT DISTINCT owner_id FROM edges WHERE owner_id IS NOT NULL")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;

        let mut rewrites = Vec::new();
        for row in rows {
            let owner = row?;
            let Some((prefix, rest)) = owner.split_once(':') else {
                continue;
            };
            if !is_absolute_path(rest) {
                continue;
            }
            if let Some(relative) = self.longest_known_suffix(rest, prefix, "")? {
                let to = format!("{prefix}:{relative}");
                if to != owner {
                    rewrites.push((owner, to));
                }
            }
        }
        for (from, to) in rewrites {
            self.conn.execute(
                "UPDATE edges SET owner_id = ?2 WHERE owner_id = ?1",
                params![from, to],
            )?;
        }
        Ok(())
    }
}
