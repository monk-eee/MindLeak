use std::collections::HashMap;

use crate::ingest::structure::{HierarchyRelation, ImportTarget};
use crate::{
    ingest, now_unix, ArtifactStub, Edge, MindLeak, MindLeakError, Node, NodeType, RelationType,
    Result, WriteOutcome,
};

impl MindLeak {
    /// Replace a source file's authoritative structural snapshot.
    pub fn ingest_file(&self, path: &str, content: &str) -> Result<WriteOutcome> {
        self.ingest_file_inner(path, content, None)
    }

    pub fn ingest_file_for_agent(
        &self,
        agent: &str,
        path: &str,
        content: &str,
    ) -> Result<WriteOutcome> {
        self.ingest_file_inner(path, content, Some(agent))
    }

    fn ingest_file_inner(
        &self,
        path: &str,
        content: &str,
        agent: Option<&str>,
    ) -> Result<WriteOutcome> {
        let now = now_unix();
        let norm = self.repo_relative(path);
        // VCS internals, dependency caches, and build/test output are not source
        // and only pollute the graph with structure for paths that vanish.
        if ingest::is_ignored_path(&norm) {
            return Ok(WriteOutcome::default());
        }
        // Still absolute after `repo_relative` means this file belongs to another
        // checkout of the repository. Every worktree shares one graph (ADR-0038),
        // so minting an id here would give one file a second identity and split
        // its history, reinforcement, overlap detection, and governance. Refuse
        // loudly: the caller knows its own root and can send a relative path,
        // whereas a duplicate id is silent and only the repair pass ever sees it.
        if ingest::is_absolute_path(&norm) {
            return Err(MindLeakError::Other(format!(
                "ingest path must be repository-relative, got {norm}; it resolves \
                 outside this server's workspace root, so it would create a second \
                 identity for a file this graph already tracks"
            )));
        }
        let art_id = format!("artifact:{norm}");
        let art = Node::new(&art_id, NodeType::Artifact, norm.clone(), now);
        let mut nodes = vec![art];
        let mut edges = Vec::new();
        let mut artifact_stubs = Vec::new();
        let mut imported_symbols: HashMap<String, (String, String)> = HashMap::new();

        let extraction = ingest::ast::extract(path, content);
        let mut local_symbols: HashMap<&str, Option<&str>> = HashMap::new();
        for symbol in &extraction.symbols {
            local_symbols
                .entry(symbol.name.as_str())
                .and_modify(|identity| *identity = None)
                .or_insert(Some(symbol.qualified_name.as_str()));
        }
        for sym in &extraction.symbols {
            let sym_id = format!("symbol:{norm}:{}", sym.qualified_name);
            let label = format!("{} ({})", sym.qualified_name, sym.kind);
            // `path:line` locates the symbol; the declaration and its doc
            // comment are what give the embedding something to mean. Without
            // them a symbol embeds as its name alone, and terse implementation
            // names lose to verbose test names every time (ADR-0008).
            let context = ingest::ast::symbol_context(content, sym.line);
            let body = if context.is_empty() {
                format!("{}:{}", norm, sym.line)
            } else {
                format!("{}:{}\n{context}", norm, sym.line)
            };
            let node = Node::new(&sym_id, NodeType::Symbol, label, now).with_content(body);
            nodes.push(node);
            edges.push(Edge::new(&art_id, &sym_id, RelationType::Contains, now));
        }

        // In-file call edges (symbol -> symbol); both endpoints exist as nodes.
        for call in &extraction.calls {
            let from = format!("symbol:{norm}:{}", call.caller);
            let to = format!("symbol:{norm}:{}", call.callee);
            edges.push(Edge::new(&from, &to, RelationType::Calls, now));
        }

        for import in ingest::structure::extract(path, content) {
            let target = match import.target {
                ImportTarget::RustCrate { name, segments } => self
                    .store
                    .rust_crate_candidates(&norm, &name, &segments)?
                    .map(ImportTarget::ArtifactCandidates)
                    .unwrap_or(ImportTarget::Package(name)),
                target => target,
            };
            let target_id = match target {
                ImportTarget::ArtifactCandidates(candidates) => {
                    let known = self.store.resolve_artifact_candidate(&candidates)?;
                    let is_stub = known.is_none();
                    let Some(target_path) = known.or_else(|| candidates.first().cloned()) else {
                        continue;
                    };
                    let target_id = format!("artifact:{target_path}");
                    if is_stub {
                        artifact_stubs.push(ArtifactStub {
                            node_id: target_id.clone(),
                            candidate_ids: candidates
                                .iter()
                                .map(|path| format!("artifact:{path}"))
                                .collect(),
                        });
                    }
                    nodes.push(Node::new(
                        &target_id,
                        NodeType::Artifact,
                        target_path.clone(),
                        now,
                    ));
                    for binding in import.bindings {
                        if binding.imported != "default" && binding.imported != "*" {
                            imported_symbols
                                .insert(binding.local, (target_path.clone(), binding.imported));
                        }
                    }
                    target_id
                }
                ImportTarget::RustCrate { .. } => unreachable!("resolved above"),
                ImportTarget::Package(package) => {
                    let target_id = format!("package:{package}");
                    nodes.push(Node::new(
                        &target_id,
                        NodeType::Package,
                        package.clone(),
                        now,
                    ));
                    target_id
                }
            };
            edges.push(Edge::new(&art_id, target_id, RelationType::Imports, now));
        }

        for hierarchy in ingest::structure::extract_hierarchy(path, content) {
            let Some(source_name) = local_symbols
                .get(hierarchy.source.as_str())
                .and_then(|identity| *identity)
            else {
                continue;
            };
            let (target_path, target_name) = if let Some(target_name) = local_symbols
                .get(hierarchy.target.as_str())
                .and_then(|identity| *identity)
            {
                (norm.as_str(), target_name)
            } else if let Some((path, name)) = imported_symbols.get(&hierarchy.target) {
                (path.as_str(), name.as_str())
            } else {
                continue;
            };
            let source_id = format!("symbol:{norm}:{source_name}");
            let target_id = format!("symbol:{target_path}:{target_name}");
            if !self.store.node_exists(&target_id)? {
                nodes.push(Node::new(
                    &target_id,
                    NodeType::Symbol,
                    format!("{target_name} (imported)"),
                    now,
                ));
            }
            let relation = match hierarchy.relation {
                HierarchyRelation::Extends => RelationType::Extends,
                HierarchyRelation::Implements => RelationType::Implements,
            };
            edges.push(Edge::new(source_id, target_id, relation, now));
        }

        if let Some(rust_crate) = ingest::manifest::rust_crate(path, content)? {
            let package_id = format!("package:{}", rust_crate.name);
            let root_id = format!("artifact:{}", rust_crate.root_path);
            nodes.push(Node::new(
                &package_id,
                NodeType::Package,
                rust_crate.name,
                now,
            ));
            nodes.push(Node::new(
                &root_id,
                NodeType::Artifact,
                rust_crate.root_path.clone(),
                now,
            ));
            edges.push(Edge::new(&art_id, &package_id, RelationType::Contains, now));
            edges.push(Edge::new(&art_id, &root_id, RelationType::Contains, now));
            artifact_stubs.push(ArtifactStub {
                node_id: root_id.clone(),
                candidate_ids: vec![root_id],
            });
        }

        if let Some(dependencies) = ingest::manifest::extract(path, content)? {
            for dependency in dependencies {
                let target_id = format!("package:{}", dependency.name);
                nodes.push(Node::new(
                    &target_id,
                    NodeType::Package,
                    dependency.name.clone(),
                    now,
                ));
                edges.push(Edge::new(&art_id, target_id, RelationType::DependsOn, now));
                let Some(local_manifest) = dependency.local_manifest else {
                    continue;
                };
                let declaration_id = format!("symbol:{norm}:dependency:{}", dependency.import_name);
                let manifest_id = format!("artifact:{local_manifest}");
                nodes.push(
                    Node::new(
                        &declaration_id,
                        NodeType::Symbol,
                        format!("{} (Cargo dependency)", dependency.import_name),
                        now,
                    )
                    .with_content(dependency.name),
                );
                nodes.push(Node::new(
                    &manifest_id,
                    NodeType::Artifact,
                    local_manifest.clone(),
                    now,
                ));
                edges.push(Edge::new(
                    &art_id,
                    &declaration_id,
                    RelationType::Contains,
                    now,
                ));
                edges.push(Edge::new(
                    &declaration_id,
                    &manifest_id,
                    RelationType::DependsOn,
                    now,
                ));
                artifact_stubs.push(ArtifactStub {
                    node_id: manifest_id.clone(),
                    candidate_ids: vec![manifest_id],
                });
            }
        }

        for reference in &extraction.call_references {
            let Some((target_path, imported_name)) = imported_symbols.get(&reference.callee) else {
                continue;
            };
            let from = format!("symbol:{norm}:{}", reference.caller);
            let to = format!("symbol:{target_path}:{imported_name}");
            if !self.store.node_exists(&to)? {
                nodes.push(Node::new(
                    &to,
                    NodeType::Symbol,
                    format!("{imported_name} (imported)"),
                    now,
                ));
            }
            edges.push(Edge::new(from, to, RelationType::Calls, now));
        }

        let mut outcome = self
            .store
            .replace_structure(&art_id, &nodes, &edges, &artifact_stubs)?;
        outcome.node_ids.push(art_id.clone());
        if let Some(agent) = agent {
            self.observe(agent, &outcome.node_ids, now)?;
        }
        Ok(outcome)
    }
}
