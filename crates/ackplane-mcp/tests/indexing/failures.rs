use serde_json::json;

use super::{
    bounded,
    fixture::Fixture,
    model::{Mode, ModelServer, UNTRUSTED_BODY},
    process::ToolReply,
    MODEL,
};

fn assert_embedding_failure(reply: &ToolReply, hint: Option<&str>) {
    assert!(
        reply.is_error,
        "model failure must yield MCP isError=true; observed status={}, indexed={}",
        reply.progress["status"], reply.progress["indexed"]
    );
    assert_eq!(reply.progress["status"], "incomplete");
    assert_eq!(reply.progress["model"], MODEL);
    assert_eq!(reply.progress["indexed"], 0);
    assert_eq!(reply.progress["attempted"], 0);
    assert_eq!(reply.progress["stale_sources"], 0);
    assert!(reply.progress["remaining"].is_null());
    assert_eq!(reply.progress["failure"]["stage"], "embedding");
    let message = reply.progress["failure"]["message"].as_str().unwrap();
    assert!(!message.is_empty(), "failure must provide a useful message");
    if let Some(hint) = hint {
        assert!(message.contains(hint), "failure should identify {hint}");
    }
    assert!(
        !reply.progress.to_string().contains(UNTRUSTED_BODY),
        "untrusted model response content must not appear in progress"
    );
}

async fn check_model_failure(mode: Mode, hint: Option<&str>) {
    bounded(async {
        let Some(fixture) = Fixture::new(2).await else {
            return;
        };
        let model = ModelServer::start(mode).await;
        let replies = fixture.index(&model.url, MODEL, &[json!({})]).await;
        let stored = fixture.embeddings().await;
        let request_count = model.requests().len();
        model.stop().await;
        fixture.stop().await;
        assert_eq!(request_count, 1);
        assert_embedding_failure(&replies[0], hint);
        assert!(
            stored.is_empty(),
            "a rejected batch must publish no embedding"
        );
    })
    .await;
}

#[tokio::test]
async fn malformed_explicit_index_refuses_entire_batch() {
    check_model_failure(Mode::MalformedIndex, Some("index")).await;
}

#[tokio::test]
async fn wrong_dimensions_refuse_entire_batch() {
    check_model_failure(Mode::WrongDimensions, None).await;
}

#[tokio::test]
async fn unavailable_model_returns_actionable_progress_without_publishing() {
    check_model_failure(Mode::Unavailable, Some("MINDLEAK_EMBED_URL")).await;
}

#[tokio::test]
async fn oversized_model_body_is_bounded_and_redacted() {
    check_model_failure(Mode::Oversized, Some("4 MiB")).await;
}

#[tokio::test]
async fn redirect_body_is_refused_without_contacting_target() {
    bounded(async {
        let Some(fixture) = Fixture::new(2).await else {
            return;
        };
        let target = ModelServer::start(Mode::Valid).await;
        let model = ModelServer::start(Mode::Redirect(format!("{}/embeddings", target.url))).await;
        let replies = fixture.index(&model.url, MODEL, &[json!({})]).await;
        let stored = fixture.embeddings().await;
        let source_contacts = model.requests().len();
        let target_contacts = target.requests().len();
        model.stop().await;
        target.stop().await;
        fixture.stop().await;
        assert_eq!(source_contacts, 1);
        assert_eq!(target_contacts, 0, "HTTP redirects must never be followed");
        assert!(replies[0].is_error,
            "HTTP 302 with a valid-looking body must be refused: isError={}, status={}, indexed={}, stored={}",
            replies[0].is_error, replies[0].progress["status"], replies[0].progress["indexed"], stored.len());
        assert_embedding_failure(&replies[0], None);
        assert!(stored.is_empty(), "a redirect body must not become durable vectors");
    }).await;
}
