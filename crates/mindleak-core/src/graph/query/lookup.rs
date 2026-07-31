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

    /// Resolve a Rust crate import through a real root declared by Cargo.
    ///
    /// More than one matching root is ambiguous and therefore unresolved: a
    /// missing impact edge is preferable to a confidently false one.
    pub fn rust_crate_candidates(
        &self,
        importer_path: &str,
        crate_name: &str,
        segments: &[String],
    ) -> Result<Option<Vec<String>>> {
        let manifest_candidates = cargo_manifest_candidates(importer_path);
        let Some(importer_manifest) = self.resolve_artifact_candidate(&manifest_candidates)? else {
            return Ok(None);
        };
        let dependency_id = format!("symbol:{importer_manifest}:dependency:{crate_name}");
        let mut statement = self.conn.prepare(
            "SELECT DISTINCT root.id
             FROM edges dependency_edge
             JOIN edges root_edge ON root_edge.source_id = dependency_edge.target_id
             JOIN nodes root ON root.id = root_edge.target_id
             WHERE dependency_edge.source_id = ?1
               AND dependency_edge.relation = 'depends_on'
               AND dependency_edge.owner_id = ?2
               AND root_edge.relation = 'contains'
               AND root_edge.owner_id = dependency_edge.target_id
               AND root.type = 'artifact'
               AND NOT EXISTS (
                   SELECT 1 FROM artifact_stubs stub WHERE stub.node_id = root.id
               )",
        )?;
        let importer_id = format!("artifact:{importer_manifest}");
        let rows = statement.query_map(params![dependency_id, importer_id], |row| {
            row.get::<_, String>(0)
        })?;
        let mut roots = std::collections::BTreeSet::new();
        for row in rows {
            roots.insert(row?);
        }
        if roots.len() != 1 {
            return Ok(None);
        }
        let root_id = roots.into_iter().next().unwrap_or_default();
        let root_path = root_id.strip_prefix("artifact:").unwrap_or(&root_id);
        let directory = root_path
            .rsplit_once('/')
            .map(|(directory, _)| format!("{directory}/"))
            .unwrap_or_default();
        let mut candidates = Vec::new();
        for end in (1..=segments.len()).rev() {
            let module = segments[..end].join("/");
            candidates.push(format!("{directory}{module}.rs"));
            candidates.push(format!("{directory}{module}/mod.rs"));
        }
        candidates.push(root_path.to_string());
        candidates.dedup();
        Ok(Some(candidates))
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

fn cargo_manifest_candidates(path: &str) -> Vec<String> {
    let mut directory = path.rsplit_once('/').map(|(directory, _)| directory);
    let mut candidates = Vec::new();
    while let Some(current) = directory {
        candidates.push(format!("{current}/Cargo.toml"));
        directory = current.rsplit_once('/').map(|(parent, _)| parent);
    }
    candidates.push("Cargo.toml".to_string());
    candidates
}
