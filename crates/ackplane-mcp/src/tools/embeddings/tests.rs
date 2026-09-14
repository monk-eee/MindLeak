use std::collections::VecDeque;

use super::*;

#[derive(Default)]
struct FakeBackend {
    pages: VecDeque<v1::ListMissingProjectionEmbeddingsResult>,
    vectors: Option<Vec<Vec<f32>>>,
    embed_error: bool,
    publish_outcomes: VecDeque<Result<bool, String>>,
    embedded: Vec<Vec<String>>,
    published: Vec<String>,
}

impl Backend for FakeBackend {
    fn missing(
        &mut self,
        _model: &str,
        _limit: u32,
    ) -> Result<v1::ListMissingProjectionEmbeddingsResult, String> {
        Ok(self.pages.pop_front().unwrap_or_default())
    }
    fn embed(&mut self, labels: &[String]) -> Result<Vec<Vec<f32>>, String> {
        self.embedded.push(labels.to_vec());
        if self.embed_error {
            return Err("model unavailable".into());
        }
        Ok(self
            .vectors
            .clone()
            .unwrap_or_else(|| vec![vec![0.25; 768]; labels.len()]))
    }
    fn publish(
        &mut self,
        source: &v1::ProjectionEmbeddingSource,
        _model: &str,
        _vector: Vec<f32>,
    ) -> Result<bool, String> {
        self.published.push(source.node_id.clone());
        self.publish_outcomes.pop_front().unwrap_or(Ok(true))
    }
}

fn page(count: usize, has_more: bool) -> v1::ListMissingProjectionEmbeddingsResult {
    v1::ListMissingProjectionEmbeddingsResult {
        nodes: (0..count)
            .map(|index| v1::ProjectionEmbeddingSource {
                node_id: format!("artifact:{index}"),
                label: format!("label {index}"),
            })
            .collect(),
        has_more,
    }
}

#[test]
fn arguments_are_strict_and_bounded() {
    assert_eq!(parse_limit(&json!({})).unwrap(), DEFAULT_LIMIT);
    for value in [
        json!({"limit": 0}),
        json!({"limit": -1}),
        json!({"limit": 1.5}),
        json!({"limit": 1001}),
        json!({"limit": "2"}),
        json!({"model": "override"}),
    ] {
        assert!(parse_limit(&value).is_err());
    }
}

#[test]
fn an_empty_queue_makes_no_embedding_request() {
    let mut backend = FakeBackend::default();
    let result = run(&mut backend, "model", 20).unwrap();
    assert_eq!(result["status"], "complete");
    assert_eq!(result["remaining"], false);
    assert!(backend.embedded.is_empty());
    assert!(backend.published.is_empty());
}

#[test]
fn index_embeds_only_labels_and_reports_confirmed_writes() {
    let mut backend = FakeBackend {
        pages: VecDeque::from([page(2, false)]),
        ..Default::default()
    };
    let result = run(&mut backend, "model", 20).unwrap();
    assert_eq!(result["indexed"], 2);
    assert_eq!(result["attempted"], 2);
    assert_eq!(result["status"], "complete");
    assert_eq!(
        backend.embedded,
        vec![vec!["label 0".to_string(), "label 1".to_string()]]
    );
}

#[test]
fn a_stale_repeated_source_stops_without_reembedding_forever() {
    let mut backend = FakeBackend {
        pages: VecDeque::from([page(1, false), page(1, false)]),
        publish_outcomes: VecDeque::from([Ok(false)]),
        ..Default::default()
    };
    let result = run(&mut backend, "model", 20).unwrap();
    assert_eq!(result["stale_sources"], 1);
    assert_eq!(result["status"], "source_changed");
    assert_eq!(backend.embedded.len(), 1);
}

#[test]
fn invalid_vectors_refuse_the_whole_batch_before_any_publication() {
    for vectors in [
        vec![vec![0.25; 768]],
        vec![vec![0.25; 768], vec![0.25; 2]],
        vec![vec![0.25; 768], vec![0.0; 768]],
    ] {
        let mut backend = FakeBackend {
            pages: VecDeque::from([page(2, false)]),
            vectors: Some(vectors),
            ..Default::default()
        };
        let error: Value =
            serde_json::from_str(&run(&mut backend, "model", 20).unwrap_err()).unwrap();
        assert_eq!(error["failure"]["stage"], "embedding");
        assert!(backend.published.is_empty());
    }
}

#[test]
fn model_failure_and_ambiguous_publication_preserve_progress() {
    let mut unavailable = FakeBackend {
        pages: VecDeque::from([page(2, false)]),
        embed_error: true,
        ..Default::default()
    };
    assert!(run(&mut unavailable, "model", 20).is_err());
    assert!(unavailable.published.is_empty());
    let mut partial = FakeBackend {
        pages: VecDeque::from([page(2, false)]),
        publish_outcomes: VecDeque::from([Ok(true), Err("outcome unknown".into())]),
        ..Default::default()
    };
    let error: Value = serde_json::from_str(&run(&mut partial, "model", 20).unwrap_err()).unwrap();
    assert_eq!(error["indexed"], 1);
    assert_eq!(error["attempted"], 2);
    assert!(error["remaining"].is_null());
    assert_eq!(error["failure"]["stage"], "publish");
}

#[test]
fn attempt_limit_preserves_a_remaining_work_signal() {
    let mut backend = FakeBackend {
        pages: VecDeque::from([page(2, true)]),
        ..Default::default()
    };
    let result = run(&mut backend, "model", 2).unwrap();
    assert_eq!(result["status"], "limited");
    assert_eq!(result["remaining"], true);
    assert_eq!(backend.published.len(), 2);
}
