use ackplane_client::companion::wire::Operation;
use ackplane_protocol::{projection_embedding_auth::ProjectionEmbeddingOperation, v1};
use serde_json::{json, Value};

use super::NodeBackend;

const FLOOR: f32 = 0.5;

pub(in crate::tools) fn definition() -> Value {
    json!({
        "name": crate::tools::RECALL,
        "description": "Recall shared projected nodes using server-side cosine, kind-prior and distinctive-field ranking. Returns explicit projection freshness and model coverage; an empty result distinguishes no match from empty, unprojected, stale or unembedded data. A state probe precedes optional node-side query embedding. Uses the enrolled companion; never searches local SQLite or computes embeddings inside Ackplane.",
        "inputSchema": {"type": "object", "properties": {
            "query": {"type": "string", "minLength": 1, "maxLength": 4096},
            "limit": {"type": "integer", "default": 10, "minimum": 1, "maximum": 100}
        }, "required": ["query"], "additionalProperties": false}
    })
}

pub(in crate::tools) fn call(
    endpoint: &str,
    arguments: &Value,
    environment: &impl Fn(&str) -> Option<String>,
) -> Result<Value, String> {
    let (query, limit) = arguments_for(arguments)?;
    let backend = NodeBackend::new(endpoint, environment)?;
    let probe = backend
        .recall_snapshot(vec![], limit)
        .map_err(|message| unavailable("sources", message, None))?;
    let observed = render(&probe, &backend.embedder.model, limit)?;
    if probe.searched || !probe.hits.is_empty() {
        return Err("a recall status probe must not return searched results".into());
    }
    if !matches!(
        probe.state(),
        v1::ProjectionRecallState::Current | v1::ProjectionRecallState::PartiallyEmbedded
    ) {
        return Ok(observed);
    }
    let query_vector = backend
        .embedder
        .embed(&[query])
        .map_err(|message| unavailable("embedding", message, Some(&observed)))?
        .pop()
        .ok_or_else(|| {
            unavailable(
                "embedding",
                "no query vector returned".into(),
                Some(&observed),
            )
        })?;
    ProjectionEmbeddingOperation::Recall {
        model: &backend.embedder.model,
        query_embedding: &query_vector,
        floor: FLOOR,
        limit,
    }
    .validate()
    .map_err(|message| unavailable("embedding", message.into(), Some(&observed)))?;
    let reply = backend
        .recall_snapshot(query_vector, limit)
        .map_err(|message| unavailable("sources", message, Some(&observed)))?;
    render(&reply, &backend.embedder.model, limit)
}

impl NodeBackend {
    fn recall_snapshot(
        &self,
        query_embedding: Vec<f32>,
        limit: u32,
    ) -> Result<v1::RecallProjectedNodesResult, String> {
        self.runtime
            .block_on(self.node.protobuf(Operation::ProjectionRecall {
                model: self.embedder.model.clone(),
                query_embedding,
                floor: FLOOR,
                limit,
            }))
            .map_err(|_| "could not read shared recall through the enrolled companion".to_string())
    }
}

fn arguments_for(arguments: &Value) -> Result<(String, u32), String> {
    let object = arguments
        .as_object()
        .ok_or("recall arguments must be an object")?;
    if object.keys().any(|key| key != "query" && key != "limit") {
        return Err("recall accepts only query and limit".into());
    }
    let query = object
        .get("query")
        .and_then(Value::as_str)
        .filter(|query| !query.trim().is_empty() && query.len() <= 4096)
        .ok_or("recall query must contain 1 to 4096 bytes")?;
    let limit = match object.get("limit") {
        None => 10,
        Some(value) => value
            .as_u64()
            .filter(|limit| (1..=100).contains(limit))
            .ok_or("recall limit must be an integer from 1 to 100")? as u32,
    };
    Ok((query.to_string(), limit))
}

fn render(
    reply: &v1::RecallProjectedNodesResult,
    model: &str,
    limit: u32,
) -> Result<Value, String> {
    let state = match v1::ProjectionRecallState::try_from(reply.state) {
        Ok(v1::ProjectionRecallState::Empty) => "empty",
        Ok(v1::ProjectionRecallState::NotYetProjected) => "not_yet_projected",
        Ok(v1::ProjectionRecallState::Stale) => "stale",
        Ok(v1::ProjectionRecallState::NotYetEmbedded) => "not_yet_embedded",
        Ok(v1::ProjectionRecallState::PartiallyEmbedded) => "partially_embedded",
        Ok(v1::ProjectionRecallState::Current) => "current",
        Ok(v1::ProjectionRecallState::Unspecified) | Err(_) => {
            return Err("shared recall returned an unknown state".into());
        }
    };
    if reply.model != model
        || reply.hits.len() > limit as usize
        || reply.embedded_nodes > reply.projected_nodes
        || (!reply.searched && !reply.hits.is_empty())
        || reply.hits.iter().any(|hit| !hit.similarity.is_finite())
    {
        return Err("shared recall returned inconsistent metadata".into());
    }
    Ok(json!({
        "state": state, "model": reply.model, "searched": reply.searched,
        "results": reply.hits.iter().map(|hit| json!({
            "id": hit.node_id, "label": hit.label, "node_type": hit.node_type, "score": hit.similarity
        })).collect::<Vec<_>>(),
        "coverage": {"projected_nodes": reply.projected_nodes, "embedded_nodes": reply.embedded_nodes},
        "projection": {"ledger_position": reply.ledger_position,
            "projected_position": reply.projected_position, "projected_at": reply.projected_at},
        "truncated": reply.truncated,
    }))
}

fn unavailable(stage: &str, message: String, observed: Option<&Value>) -> String {
    json!({"state": "unavailable", "searched": false, "results": [],
        "failure": {"stage": stage, "message": message}, "last_observed": observed})
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_arguments_are_strict_and_bounded() {
        assert_eq!(
            arguments_for(&json!({"query": "why"})).unwrap(),
            ("why".into(), 10)
        );
        for arguments in [
            json!({}),
            json!({"query": " "}),
            json!({"query": "x".repeat(4097)}),
            json!({"query": "why", "limit": 0}),
            json!({"query": "why", "limit": 101}),
            json!({"query": "why", "limit": 1.5}),
            json!({"query": "why", "model": "override"}),
        ] {
            assert!(arguments_for(&arguments).is_err());
        }
    }

    #[test]
    fn empty_answers_preserve_their_state_and_absent_checkpoint() {
        for state in [
            v1::ProjectionRecallState::Empty,
            v1::ProjectionRecallState::NotYetProjected,
            v1::ProjectionRecallState::Stale,
            v1::ProjectionRecallState::NotYetEmbedded,
            v1::ProjectionRecallState::PartiallyEmbedded,
            v1::ProjectionRecallState::Current,
        ] {
            let reply = v1::RecallProjectedNodesResult {
                state: state as i32,
                model: "model".into(),
                ..Default::default()
            };
            let rendered = render(&reply, "model", 10).unwrap();
            assert!(rendered["results"].as_array().unwrap().is_empty());
            assert!(rendered["projection"]["projected_position"].is_null());
            assert_eq!(rendered["searched"], false);
            assert_ne!(rendered["state"], "unavailable");
        }
    }

    #[test]
    fn a_model_failure_is_not_a_successful_no_match() {
        let observed = json!({"state": "current", "searched": false});
        let failure: Value = serde_json::from_str(&unavailable(
            "embedding",
            "unreachable".into(),
            Some(&observed),
        ))
        .unwrap();
        assert_eq!(failure["state"], "unavailable");
        assert_eq!(failure["last_observed"], observed);
        assert_eq!(failure["searched"], false);
    }

    #[test]
    fn unknown_state_wrong_model_and_unsearched_hits_are_refused() {
        let mut reply = v1::RecallProjectedNodesResult {
            model: "model".into(),
            ..Default::default()
        };
        assert!(render(&reply, "model", 10).is_err());
        reply.state = v1::ProjectionRecallState::Current as i32;
        assert!(render(&reply, "other", 10).is_err());
        reply.hits.push(v1::ProjectionRecallHit::default());
        assert!(render(&reply, "model", 10).is_err());
    }
}
