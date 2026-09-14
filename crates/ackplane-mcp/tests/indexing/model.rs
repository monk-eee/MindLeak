use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use axum::{
    extract::State,
    http::{header, HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
    Json, Router,
};
use serde_json::{json, Value};
use tokio::{net::TcpListener, sync::oneshot};

use super::{service::RunningServer, API_KEY};

pub const UNTRUSTED_BODY: &str = "untrusted-model-response-body-sentinel";

pub enum Mode {
    Valid,
    Vectors(BTreeMap<String, Vec<f32>>),
    MalformedIndex,
    WrongDimensions,
    Unavailable,
    Redirect(String),
    Oversized,
}

#[derive(Clone)]
pub struct ModelRequest {
    pub body: Value,
    pub authorization: Option<String>,
    pub method: Method,
}

struct ModelState {
    mode: Mode,
    requests: Mutex<Vec<ModelRequest>>,
}

pub struct ModelServer {
    pub url: String,
    state: Arc<ModelState>,
    server: RunningServer,
}

impl ModelServer {
    pub async fn start(mode: Mode) -> Self {
        let state = Arc::new(ModelState {
            mode,
            requests: Mutex::new(Vec::new()),
        });
        let app = Router::new()
            .route("/v1/embeddings", any(embed))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let (shutdown, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .expect("the fixture model HTTP server must run");
        });
        Self {
            url,
            state,
            server: RunningServer::new(shutdown, task),
        }
    }

    pub fn requests(&self) -> Vec<ModelRequest> {
        self.state.requests.lock().unwrap().clone()
    }

    pub async fn stop(self) {
        self.server.stop().await;
    }
}

async fn embed(
    State(state): State<Arc<ModelState>>,
    method: Method,
    headers: HeaderMap,
    body: Option<Json<Value>>,
) -> Response {
    let body = body.map_or(Value::Null, |Json(body)| body);
    state.requests.lock().unwrap().push(ModelRequest {
        body: body.clone(),
        authorization: headers
            .get("authorization")
            .map(|header| header.to_str().unwrap().to_string()),
        method: method.clone(),
    });
    if method != Method::POST {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let Some(inputs) = body["input"].as_array() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let mut data: Vec<_> = inputs
        .iter()
        .enumerate()
        .rev()
        .map(|(index, label)| {
            let label = label.as_str().unwrap();
            let vector = match &state.mode {
                Mode::Vectors(vectors) => vectors
                    .get(label)
                    .expect("a configured fixture input")
                    .clone(),
                _ => vector_for_label(label),
            };
            json!({"index": index, "embedding": vector})
        })
        .collect();
    match &state.mode {
        Mode::Valid | Mode::Vectors(_) => Json(json!({"data": data})).into_response(),
        Mode::MalformedIndex => {
            data[0]["index"] = json!(-1);
            Json(json!({"data": data, "debug": format!("{UNTRUSTED_BODY} {API_KEY}")}))
                .into_response()
        }
        Mode::WrongDimensions => {
            data[0]["embedding"] = json!(vec![1.0; 767]);
            Json(json!({"data": data, "debug": format!("{UNTRUSTED_BODY} {API_KEY}")}))
                .into_response()
        }
        Mode::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("{UNTRUSTED_BODY} {API_KEY}"),
        )
            .into_response(),
        Mode::Redirect(location) => (
            StatusCode::FOUND,
            [(header::LOCATION, location.as_str())],
            Json(json!({"data": data})),
        )
            .into_response(),
        Mode::Oversized => Json(json!({
            "data": data,
            "padding": format!("{UNTRUSTED_BODY} {API_KEY} {}", "x".repeat(4 * 1024 * 1024)),
        }))
        .into_response(),
    }
}

pub fn vector_for_label(label: &str) -> Vec<f32> {
    let mut vector = vec![0.0; 768];
    vector[0] = 1.0;
    for (offset, byte) in label.bytes().enumerate() {
        vector[1 + offset % 767] += f32::from(byte);
    }
    vector
}
