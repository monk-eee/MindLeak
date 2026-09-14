use std::{
    io::Write,
    process::{Command, Output},
};

use serde_json::Value;

fn preview(bytes: &[u8], origin: &str, repository: &str) -> Output {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(bytes).unwrap();
    Command::new(env!("CARGO_BIN_EXE_ackplane-workctl"))
        .args([
            "design",
            "preview",
            "--bridge-url",
            origin,
            "--repository-id",
            repository,
            "--file",
            file.path().to_str().unwrap(),
        ])
        .output()
        .unwrap()
}

#[test]
fn design_preview_renders_the_exact_proposal_without_contacting_the_bridge() {
    let result = Command::new(env!("CARGO_BIN_EXE_ackplane-workctl"))
        .args([
            "design",
            "preview",
            "--bridge-url",
            "http://127.0.0.1:1",
            "--repository-id",
            "repository:conversation",
            "--file",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/conversation_design.json"
            ),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let preview: Value = serde_json::from_slice(&result.stdout).unwrap();
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/conversation_design.json")).unwrap();
    assert_eq!(preview["operation"], "propose_design");
    assert_eq!(preview["repository_id"], "repository:conversation");
    assert_eq!(preview["proposal"], expected);
    assert_eq!(preview["confirmation_digest"].as_str().unwrap().len(), 64);
    assert_eq!(preview["persisted"], false);
}

#[test]
fn unconfirmed_publication_is_refused_before_connecting() {
    let result = Command::new(env!("CARGO_BIN_EXE_ackplane-workctl"))
        .args([
            "design",
            "propose",
            "--bridge-url",
            "http://127.0.0.1:1",
            "--repository-id",
            "repository:conversation",
            "--file",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/conversation_design.json"
            ),
            "--confirm-digest",
            "wrong",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("confirmation does not match"));
    assert!(result.stdout.is_empty());
}

#[test]
fn malformed_oversized_and_identity_forging_proposals_are_refused() {
    let original: Value =
        serde_json::from_str(include_str!("fixtures/conversation_design.json")).unwrap();
    for field in ["actor", "proposed_by", "lifecycle_state", "approved_by"] {
        let mut proposal = original.clone();
        proposal[field] = Value::String("not-authority".into());
        let result = preview(
            &serde_json::to_vec(&proposal).unwrap(),
            "http://127.0.0.1:1",
            "repository:a",
        );
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("unknown field"));
    }
    for bytes in [b"{broken".to_vec(), vec![b'x'; 65537]] {
        assert!(!preview(&bytes, "http://127.0.0.1:1", "repository:a")
            .status
            .success());
    }
    for field in ["title", "summary", "design_id", "source_version"] {
        let mut proposal = original.clone();
        proposal[field] = Value::String(" ".into());
        assert!(!preview(
            &serde_json::to_vec(&proposal).unwrap(),
            "http://127.0.0.1:1",
            "repository:a"
        )
        .status
        .success());
    }
}

#[test]
fn review_digest_binds_content_and_target_but_not_json_formatting() {
    let bytes = include_bytes!("fixtures/conversation_design.json");
    let mut document: Value = serde_json::from_slice(bytes).unwrap();
    let digest = |bytes: &[u8], origin: &str, repository: &str| -> String {
        let result = preview(bytes, origin, repository);
        assert!(result.status.success());
        serde_json::from_slice::<Value>(&result.stdout).unwrap()["confirmation_digest"]
            .as_str()
            .unwrap()
            .into()
    };
    let original = digest(bytes, "http://127.0.0.1:1", "repository:a");
    assert_eq!(
        original,
        digest(
            &serde_json::to_vec(&document).unwrap(),
            "http://127.0.0.1:1/",
            "repository:a"
        )
    );
    assert_ne!(
        original,
        digest(bytes, "http://127.0.0.1:2", "repository:a")
    );
    assert_ne!(
        original,
        digest(bytes, "http://127.0.0.1:1", "repository:b")
    );
    document["summary"] = Value::String("Different requested behavior".into());
    assert_ne!(
        original,
        digest(
            &serde_json::to_vec(&document).unwrap(),
            "http://127.0.0.1:1",
            "repository:a"
        )
    );
}

#[test]
fn only_an_explicit_loopback_bridge_origin_can_receive_designs() {
    for origin in [
        "https://example.com",
        "http://localhost.example.com",
        "http://0.0.0.0",
        "http://localhost/path",
        "http://localhost/?query=yes",
        "http://localhost/#fragment",
        "http://private-value@localhost",
        "file:///tmp/bridge",
    ] {
        let result = preview(
            include_bytes!("fixtures/conversation_design.json"),
            origin,
            "repository:a",
        );
        assert!(!result.status.success(), "accepted {origin}");
        assert!(!String::from_utf8_lossy(&result.stderr).contains("private-value"));
    }
}

#[test]
fn dot_identifiers_are_refused_instead_of_silently_removed_from_the_target_url() {
    // URL segment construction omits dot components; accepting them can review
    // one repository identifier while sending a different request path.
    let bytes = include_bytes!("fixtures/conversation_design.json");
    for identifier in [".", ".."] {
        assert!(!preview(bytes, "http://127.0.0.1:1", identifier)
            .status
            .success());
        let mut proposal: Value = serde_json::from_slice(bytes).unwrap();
        proposal["design_id"] = Value::String(identifier.into());
        assert!(!preview(
            &serde_json::to_vec(&proposal).unwrap(),
            "http://127.0.0.1:1",
            "repository:a"
        )
        .status
        .success());
    }
}

#[tokio::test]
async fn publication_refuses_redirects_and_http_errors_and_reports_uncertain_readback() {
    use axum::{
        body::{Body, Bytes},
        http::{Method, Response, StatusCode},
        Router,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    let mode = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let handler_mode = mode.clone();
    let handler_calls = calls.clone();
    let app = Router::new().fallback(move |method: Method, body: Bytes| {
        if method == Method::POST {
            assert!(serde_json::from_slice::<Value>(&body).unwrap().is_object());
        }
        handler_calls.fetch_add(1, Ordering::SeqCst);
        let mode = handler_mode.load(Ordering::SeqCst);
        async move {
            let status = match mode {
                0 => StatusCode::TEMPORARY_REDIRECT,
                1 => StatusCode::CONFLICT,
                2 => StatusCode::INTERNAL_SERVER_ERROR,
                _ if method == Method::POST => StatusCode::CREATED,
                _ => StatusCode::OK,
            };
            Response::builder()
                .status(status)
                .header("location", "/outside")
                .body(Body::from("not JSON"))
                .unwrap()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let probe_origin = origin.clone();
    tokio::task::spawn_blocking(move || {
        use std::io::{BufRead, BufReader, Read};
        use std::net::TcpStream;
        use std::time::Duration;

        let address = probe_origin.strip_prefix("http://").unwrap();
        let mut connection = TcpStream::connect(address).unwrap();
        connection.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let body = include_bytes!("fixtures/conversation_design.json");
        write!(connection,
            "POST / HTTP/1.1\r\nHost: {address}\r\nContent-Length: {}\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n",
            body.len()
        ).unwrap();
        let mut reader = BufReader::new(connection);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        // Replying before reading the POST body raced the client's send and
        // produced a transport failure instead of the HTTP status under test.
        assert_eq!(line, "HTTP/1.1 100 Continue\r\n");
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "\r\n");
        reader.get_mut().write_all(body).unwrap();
        let mut response = String::new();
        reader.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 307"), "{response}");
    }).await.unwrap();
    let review: Value = serde_json::from_slice(
        &preview(
            include_bytes!("fixtures/conversation_design.json"),
            &origin,
            "repository:a",
        )
        .stdout,
    )
    .unwrap();
    for (index, expected) in [
        "HTTP 307",
        "HTTP 409",
        "HTTP 500",
        "proposal sent, read-back failed",
    ]
    .into_iter()
    .enumerate()
    {
        mode.store(index, Ordering::SeqCst);
        calls.store(0, Ordering::SeqCst);
        let origin = origin.clone();
        let digest = review["confirmation_digest"].as_str().unwrap().to_string();
        let output = tokio::task::spawn_blocking(move || {
            Command::new(env!("CARGO_BIN_EXE_ackplane-workctl"))
                .args([
                    "design",
                    "propose",
                    "--bridge-url",
                    &origin,
                    "--repository-id",
                    "repository:a",
                    "--file",
                    concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/tests/fixtures/conversation_design.json"
                    ),
                    "--confirm-digest",
                    &digest,
                ])
                .output()
                .unwrap()
        })
        .await
        .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(calls.load(Ordering::SeqCst), if index == 3 { 2 } else { 1 });
    }
    server.abort();
}
