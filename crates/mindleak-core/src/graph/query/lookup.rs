//! Finding nodes: exact ids, artifact-path convenience forms, and full-text seeds.
//!
//! Split out of `query.rs` (see `super`); the code is unchanged.

use super::*;

impl GraphStore {
    /// Pick the first already-ingested artifact from deterministic candidates.
    pub fn resolve_artifact_candidate(&self, candidates: &[String]) -> Result<Option<String>> {
        for path in candidates {
            let id = format!("artifact:{path}");
            let is_real: bool = self.conn.query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM nodes n
                     WHERE n.id = ?1
                       AND NOT EXISTS (
                           SELECT 1 FROM artifact_stubs s WHERE s.node_id = n.id
                       )
                 )",
                params![id],
                |row| row.get(0),
            )?;
            if is_real {
                return Ok(Some(path.clone()));
            }
        }
        Ok(None)
    }

    /// Full-text search over node labels + content. Returns best matches first.
    pub fn search_nodes(&self, query: &str, limit: usize) -> Result<Vec<Node>> {
        let match_query = build_fts_query(query);
        if match_query.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT n.id, n.type, n.label, n.content, n.created_at, n.last_accessed_at
             FROM nodes_fts f
             JOIN nodes n ON n.id = f.id
             WHERE nodes_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![match_query, limit as i64], row_to_node)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Resolve a seed argument to one or more node ids.
    /// An exact node id wins; otherwise fall back to full-text search.
    pub fn resolve_seed(&self, seed: &str, limit: usize) -> Result<Vec<String>> {
        if self.node_exists(seed)? {
            return Ok(vec![seed.to_string()]);
        }
        // Try an artifact-path convenience form (`src/x.ts` -> `artifact:src/x.ts`).
        let artifact = format!("artifact:{seed}");
        if self.node_exists(&artifact)? {
            return Ok(vec![artifact]);
        }
        let hits = self.search_nodes(seed, limit)?;
        Ok(hits.into_iter().map(|n| n.id).collect())
    }
}
