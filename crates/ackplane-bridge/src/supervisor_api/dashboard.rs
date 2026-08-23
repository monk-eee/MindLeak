//! Browser page host for the read-only ADR-0116 supervisor runtime dashboard.

use axum::{response::Html, routing::get, Router};

use super::SupervisorApiState;

const SUPERVISOR_DASHBOARD: &str = include_str!("../../static/supervisors.html");

pub(super) fn routes() -> Router<SupervisorApiState> {
    Router::new().route("/supervisors", get(supervisor_dashboard_page))
}

async fn supervisor_dashboard_page() -> Html<&'static str> {
    Html(SUPERVISOR_DASHBOARD)
}
