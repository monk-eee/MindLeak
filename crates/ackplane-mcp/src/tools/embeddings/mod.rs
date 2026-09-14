use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use ackplane_client::{
    companion::{wire::Operation, NodeClient},
    resolve_node_client,
};
use ackplane_protocol::{projection_embedding_auth::ProjectionEmbeddingOperation, v1};
use prost::Message;
use serde_json::{json, Value};

mod http;

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 1000;
const BATCH_SIZE: usize = 20;
const PASS_DURATION: Duration = Duration::from_secs(120);

pub(super) fn definition() -> Value {
    json!({
        "name": super::INDEX,
        "description": "Explicitly index up to limit projected labels for this enrolled repository. Embeddings are computed by this node-side process using MINDLEAK_EMBED_URL/MODEL/API_KEY, then signed and published by the node companion. Off the deterministic hot path; optional and bounded. Returns stored, attempted and stale-source counts plus whether another pass is needed. This does not index a local SQLite graph or certify projection freshness.",
        "inputSchema": { "type": "object", "properties": {
            "limit": { "type": "integer", "default": DEFAULT_LIMIT, "minimum": 1, "maximum": MAX_LIMIT }
        }, "additionalProperties": false }
    })
}

pub(super) fn index(
    endpoint: &str,
    arguments: &Value,
    environment: &impl Fn(&str) -> Option<String>,
) -> Result<Value, String> {
    let limit = parse_limit(arguments)?;
    let embedder = http::Embedder::from_environment(environment)?;
    let mut node = resolve_node_client(environment)
        .map_err(|_| "index requires a configured enrolled node companion".to_string())?;
    node.expected_endpoint = Some(endpoint.to_string());
    let runtime = super::runtime()?;
    let model = embedder.model.clone();
    let mut backend = NodeBackend {
        node,
        runtime,
        embedder,
    };
    run(&mut backend, &model, limit)
}

fn parse_limit(arguments: &Value) -> Result<usize, String> {
    if !arguments.is_null() && !arguments.is_object() {
        return Err("index arguments must be an object".into());
    }
    if arguments
        .as_object()
        .is_some_and(|object| object.keys().any(|key| key != "limit"))
    {
        return Err("index accepts only limit; model and identity come from configuration".into());
    }
    match arguments.get("limit") {
        None => Ok(DEFAULT_LIMIT),
        Some(value) => value
            .as_u64()
            .filter(|limit| (1..=MAX_LIMIT as u64).contains(limit))
            .map(|limit| limit as usize)
            .ok_or_else(|| "index limit must be an integer from 1 to 1000".into()),
    }
}

trait Backend {
    fn missing(
        &mut self,
        model: &str,
        limit: u32,
    ) -> Result<v1::ListMissingProjectionEmbeddingsResult, String>;
    fn embed(&mut self, labels: &[String]) -> Result<Vec<Vec<f32>>, String>;
    fn publish(
        &mut self,
        source: &v1::ProjectionEmbeddingSource,
        model: &str,
        vector: Vec<f32>,
    ) -> Result<bool, String>;
}

struct NodeBackend {
    node: NodeClient,
    runtime: tokio::runtime::Runtime,
    embedder: http::Embedder,
}

impl Backend for NodeBackend {
    fn missing(
        &mut self,
        model: &str,
        limit: u32,
    ) -> Result<v1::ListMissingProjectionEmbeddingsResult, String> {
        self.runtime
            .block_on(self.node.protobuf(Operation::ProjectionEmbeddingsMissing {
                model: model.into(),
                limit,
            }))
            .map_err(|_| "could not read projected sources through the node companion".into())
    }
    fn embed(&mut self, labels: &[String]) -> Result<Vec<Vec<f32>>, String> {
        self.embedder.embed(labels)
    }
    fn publish(
        &mut self,
        source: &v1::ProjectionEmbeddingSource,
        model: &str,
        embedding: Vec<f32>,
    ) -> Result<bool, String> {
        let reply: v1::PublishProjectionEmbeddingResult = self
            .runtime
            .block_on(self.node.protobuf(Operation::ProjectionEmbeddingPublish {
                source: source.encode_to_vec(),
                model: model.into(),
                embedding,
            }))
            .map_err(|_| {
                "embedding publication outcome is unknown; rerun index to query remaining sources"
                    .to_string()
            })?;
        Ok(reply.stored)
    }
}

#[derive(Default)]
struct Progress {
    indexed: usize,
    attempted: usize,
    stale: usize,
}

impl Progress {
    fn result(&self, model: &str, state: &str, remaining: Option<bool>) -> Value {
        json!({"model": model, "status": state, "indexed": self.indexed,
            "attempted": self.attempted, "stale_sources": self.stale, "remaining": remaining})
    }
    fn failure(&self, model: &str, stage: &str, message: String) -> String {
        let mut result = self.result(model, "incomplete", None);
        result["failure"] = json!({"stage": stage, "message": message});
        result.to_string()
    }
}

fn run(backend: &mut impl Backend, model: &str, limit: usize) -> Result<Value, String> {
    let mut progress = Progress::default();
    let mut seen = HashSet::new();
    let started = Instant::now();
    loop {
        if progress.attempted >= limit || started.elapsed() >= PASS_DURATION {
            return Ok(progress.result(model, "limited", Some(true)));
        }
        let requested = BATCH_SIZE.min(limit - progress.attempted) as u32;
        let pending = backend
            .missing(model, requested)
            .map_err(|message| progress.failure(model, "sources", message))?;
        if pending.nodes.len() > requested as usize
            || (pending.nodes.is_empty() && pending.has_more)
        {
            return Err(progress.failure(model, "sources", "invalid projected source page".into()));
        }
        if pending.nodes.is_empty() {
            return Ok(progress.result(model, "complete", Some(false)));
        }
        if pending
            .nodes
            .iter()
            .any(|source| seen.contains(&source.node_id))
        {
            return Ok(progress.result(model, "source_changed", Some(true)));
        }
        for source in &pending.nodes {
            source
                .validate()
                .map_err(|message| progress.failure(model, "sources", message.into()))?;
            if !seen.insert(source.node_id.clone()) {
                return Err(progress.failure(
                    model,
                    "sources",
                    "duplicate projected source in one page".into(),
                ));
            }
        }
        let labels: Vec<_> = pending
            .nodes
            .iter()
            .map(|source| source.label.clone())
            .collect();
        let vectors = backend
            .embed(&labels)
            .map_err(|message| progress.failure(model, "embedding", message))?;
        if vectors.len() != pending.nodes.len() {
            return Err(progress.failure(
                model,
                "embedding",
                "embedding count does not match the source batch".into(),
            ));
        }
        for (source, vector) in pending.nodes.iter().zip(&vectors) {
            ProjectionEmbeddingOperation::Publish {
                source,
                model,
                embedding: vector,
            }
            .validate()
            .map_err(|message| progress.failure(model, "embedding", message.into()))?;
        }
        for (source, vector) in pending.nodes.iter().zip(vectors) {
            if started.elapsed() >= PASS_DURATION {
                return Ok(progress.result(model, "limited", Some(true)));
            }
            progress.attempted += 1;
            if backend
                .publish(source, model, vector)
                .map_err(|message| progress.failure(model, "publish", message))?
            {
                progress.indexed += 1;
            } else {
                progress.stale += 1;
            }
        }
        if !pending.has_more && progress.stale == 0 {
            return Ok(progress.result(model, "complete", Some(false)));
        }
    }
}

#[cfg(test)]
mod tests;
