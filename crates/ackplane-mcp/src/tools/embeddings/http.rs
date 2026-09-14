use std::{io::Read, time::Duration};

use mindleak_model::embedding::parse_embedding_response;
use serde_json::{json, Value};
use url::Url;

const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

pub(super) struct Embedder {
    pub(super) model: String,
    endpoint: Url,
    api_key: String,
}

impl Embedder {
    pub(super) fn from_environment(
        environment: &impl Fn(&str) -> Option<String>,
    ) -> Result<Self, String> {
        let model =
            environment("MINDLEAK_EMBED_MODEL").unwrap_or_else(|| "nomic-embed-text".into());
        let base =
            environment("MINDLEAK_EMBED_URL").unwrap_or_else(|| "http://localhost:11434/v1".into());
        let mut endpoint = Url::parse(&base)
            .map_err(|_| "MINDLEAK_EMBED_URL must be an HTTP(S) URL".to_string())?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(
                "MINDLEAK_EMBED_URL must be HTTP(S) without user information, query or fragment"
                    .into(),
            );
        }
        ackplane_protocol::projection_embedding_auth::ProjectionEmbeddingOperation::ListMissing {
            model: &model,
            limit: 20,
        }
        .validate()
        .map_err(str::to_string)?;
        endpoint.set_path(&format!(
            "{}/embeddings",
            endpoint.path().trim_end_matches('/')
        ));
        Ok(Self {
            model,
            endpoint,
            api_key: environment("MINDLEAK_EMBED_API_KEY").unwrap_or_default(),
        })
    }

    pub(super) fn embed(&self, labels: &[String]) -> Result<Vec<Vec<f32>>, String> {
        if labels.is_empty() {
            return Ok(Vec::new());
        }
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(1))
            .timeout(Duration::from_secs(30))
            .redirects(0)
            .build();
        let mut request = agent.post(self.endpoint.as_str());
        if !self.api_key.is_empty() {
            request = request.set("authorization", &format!("Bearer {}", self.api_key));
        }
        let response = request.send_json(json!({"model": self.model, "input": labels}))
            .map_err(|error| format!(
                "embedding service unavailable ({}); check MINDLEAK_EMBED_URL and MINDLEAK_EMBED_MODEL, or start the configured embedding service",
                mindleak_model::classify_ureq_error(error).reason,
            ))?;
        if !(200..300).contains(&response.status()) {
            return Err(format!(
                "embedding service returned HTTP {}; redirects are not accepted",
                response.status()
            ));
        }
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "could not read the bounded embedding response".to_string())?;
        if bytes.len() as u64 > MAX_RESPONSE_BYTES {
            return Err("embedding response exceeds 4 MiB".into());
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| "embedding service returned invalid JSON".to_string())?;
        parse_embedding_response(&value, labels.len()).map_err(|error| error.detail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_endpoint_metadata_is_refused_without_echoing_it() {
        for endpoint in [
            "ftp://localhost/v1",
            "http://secret:secret@localhost/v1",
            "http://localhost/v1?secret=value",
            "http://localhost/v1#secret",
        ] {
            let error = Embedder::from_environment(&|name| {
                (name == "MINDLEAK_EMBED_URL").then(|| endpoint.into())
            })
            .err()
            .expect("unsafe configuration must be refused");
            assert!(!error.contains("secret"));
        }
    }

    #[test]
    fn empty_batch_never_contacts_the_model() {
        let embedder = Embedder::from_environment(&|name| {
            (name == "MINDLEAK_EMBED_URL").then(|| "http://127.0.0.1:1/v1".into())
        })
        .unwrap();
        assert!(embedder.embed(&[]).unwrap().is_empty());
    }
}
