//! Lodestar MCP — an MCP (Model Context Protocol) server over stdio that exposes
//! the Intent Plane (durable constitution, task coordination, conformance, and
//! consolidated knowledge) to coding agents.

mod server;
mod tools;

use std::path::Path;

use lodestar_core::Lodestar;

fn main() -> anyhow::Result<()> {
    let db_path = resolve_db_path();
    if let Some(parent) = Path::new(&db_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let agent = std::env::var("LODESTAR_AGENT")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let engine = Lodestar::open(&db_path)?.with_agent(agent);
    eprintln!("[lodestar-mcp] ready — intent plane at {db_path}");
    server::run(engine)
}

/// Resolve the spec database path from `LODESTAR_DB`, else `<cwd>/.lodestar/spec.db`.
///
/// A single shared file lets multiple local agents (and worktrees pointed at the
/// same path) coordinate through one Intent Plane. Full git-common-dir resolution
/// so sibling worktrees auto-share is a documented follow-up (SPEC-INTENT §3).
fn resolve_db_path() -> String {
    if let Ok(p) = std::env::var("LODESTAR_DB") {
        if !p.trim().is_empty() {
            return p;
        }
    }
    let mut base = std::env::current_dir().unwrap_or_else(|_| ".".into());
    base.push(".lodestar");
    base.push("spec.db");
    base.to_string_lossy().into_owned()
}
