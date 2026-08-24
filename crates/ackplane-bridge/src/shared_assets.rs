//! Shared static assets for the Bridge chrome (brand mark + grouped nav).
//!
//! Every static page links to these instead of inlining its own copy, so the
//! nav's structure and behaviour live in exactly one place.

use axum::{http::header::CONTENT_TYPE, response::IntoResponse, routing::get, Router};

const CHROME_CSS: &str = include_str!("../static/shared/chrome.css");
const CHROME_JS: &str = include_str!("../static/shared/chrome.js");

async fn chrome_css() -> impl IntoResponse {
    ([(CONTENT_TYPE, "text/css; charset=utf-8")], CHROME_CSS)
}

async fn chrome_js() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "text/javascript; charset=utf-8")],
        CHROME_JS,
    )
}

/// Mounted once in the application router; carries no tenant-scoped state,
/// since it serves the same bytes to every caller.
pub fn shared_asset_routes() -> Router {
    Router::new()
        .route("/static/shared/chrome.css", get(chrome_css))
        .route("/static/shared/chrome.js", get(chrome_js))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn chrome_css_is_served_as_css() {
        let response = chrome_css().await.into_response();
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "text/css; charset=utf-8"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8(body.to_vec())
            .unwrap()
            .contains(".nav-group"));
    }

    #[tokio::test]
    async fn chrome_js_is_served_as_javascript() {
        let response = chrome_js().await.into_response();
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "text/javascript; charset=utf-8"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8(body.to_vec())
            .unwrap()
            .contains("NAV_ITEMS"));
    }
}
