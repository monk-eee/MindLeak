//! mindleak-coordinator — a thin local coordinator over MindLeak and Lodestar
//! (ADR-0097): one agent-facing stdio MCP server that composes `open_session`
//! and preflight reads across both planes. It opens neither plane's database
//! directly; every tool call proxies to a spawned child of the real binary.

mod child;
mod server;
mod tools;

#[cfg(test)]
mod end_to_end_tests;

use std::path::PathBuf;

use child::SpawnedChild;

fn main() -> anyhow::Result<()> {
    let mindleak_bin = resolve_binary("MINDLEAK_MCP_BIN", "mindleak-mcp");
    let lodestar_bin = resolve_binary("LODESTAR_MCP_BIN", "lodestar-mcp");

    let mut mindleak = SpawnedChild::spawn(&mindleak_bin)
        .map_err(|e| anyhow::anyhow!("could not spawn MindLeak ({mindleak_bin}): {e}"))?;
    let mut lodestar = SpawnedChild::spawn(&lodestar_bin)
        .map_err(|e| anyhow::anyhow!("could not spawn Lodestar ({lodestar_bin}): {e}"))?;

    // Each child expects the same handshake a normal MCP client would send;
    // a plane that never completes it is a startup failure, not something
    // deferred to the first composed tool call.
    mindleak
        .client
        .initialize()
        .map_err(|e| anyhow::anyhow!("MindLeak did not answer initialize: {e}"))?;
    lodestar
        .client
        .initialize()
        .map_err(|e| anyhow::anyhow!("Lodestar did not answer initialize: {e}"))?;

    server::run(&mut mindleak.client, &mut lodestar.client)?;
    Ok(())
}

/// The env override always wins (matching `LODESTAR_MCP_BIN`/`MINDLEAK_MCP_BIN`
/// as already used by `scripts/canonical-push.mjs` and friends). Otherwise
/// prefer a sibling binary next to this executable — the common case when both
/// servers are built into the same `target/` directory — and fall back to a
/// bare command name resolved through `PATH`.
fn resolve_binary(env_var: &str, name: &str) -> String {
    if let Ok(configured) = std::env::var(env_var) {
        if !configured.trim().is_empty() {
            return configured;
        }
    }
    if let Some(sibling) = sibling_binary(name) {
        return sibling;
    }
    name.to_string()
}

fn sibling_binary(name: &str) -> Option<String> {
    let exe_name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let candidate: PathBuf = std::env::current_exe().ok()?.parent()?.join(exe_name);
    candidate.is_file().then(|| candidate.display().to_string())
}
