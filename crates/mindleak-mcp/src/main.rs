//! MindLeak MCP — an MCP (Model Context Protocol) server over stdio that exposes
//! the temporal context graph engine to coding agents.

mod server;
mod tools;

use std::path::Path;

use mindleak_core::MindLeak;

fn main() -> anyhow::Result<()> {
    let db_path = resolve_db_path();
    if let Some(parent) = Path::new(&db_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let agent = std::env::var("MINDLEAK_AGENT")
        .ok()
        .filter(|a| !a.trim().is_empty());
    let engine = MindLeak::open(&db_path)?.with_agent(agent.clone());
    match &agent {
        Some(a) => eprintln!("[mindleak-mcp] ready — graph at {db_path} (agent: {a})"),
        None => eprintln!("[mindleak-mcp] ready — graph at {db_path}"),
    }
    server::run(engine)
}

/// Resolve the graph database path from `MINDLEAK_DB`, else `<cwd>/.mindleak/graph.db`.
fn resolve_db_path() -> String {
    if let Ok(p) = std::env::var("MINDLEAK_DB") {
        if !p.trim().is_empty() {
            return p;
        }
    }
    let mut base = std::env::current_dir().unwrap_or_else(|_| ".".into());
    base.push(".mindleak");
    base.push("graph.db");
    base.to_string_lossy().into_owned()
}
