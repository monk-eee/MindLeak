//! Ackplane MCP -- an MCP server over stdio that translates tool calls into
//! Ackplane's existing typed gRPC services (ADR-0136).
//!
//! It is a protocol front door, not a second storage engine: no handler here
//! re-implements decay, claim/lease, or conformance rules, and neither
//! `mindleak-core` nor `lodestar-core` is forked onto PostgreSQL to serve it.
//! The Local profile is untouched -- `mindleak-mcp`/`lodestar-mcp` keep their
//! stdio-only, no-network-listener contract, and nothing here promotes a Local
//! repository into the Industrial one.

mod endpoint;
mod node_trust;
mod server;
mod tools;

use mindleak_session::SessionRegistry;

fn main() -> anyhow::Result<()> {
    let environment = |name: &str| std::env::var(name).ok();

    // Settled once, before serving: which arbiter this front door may reach,
    // and (ADR-0137 clause 1) whether a declared enrolled node authenticates
    // there. A refusal deliberately does not exit -- see `server::run`.
    let (endpoint, refusal) = match endpoint::resolve_endpoint(&environment) {
        Ok(endpoint) => match node_trust::establish(&endpoint, &environment) {
            Ok(()) => (Some(endpoint), None),
            Err(error) => {
                eprintln!("ackplane-mcp: {error}");
                (None, Some(error))
            }
        },
        Err(error) => {
            // stderr, never stdout: stdout is the JSON-RPC channel.
            eprintln!("ackplane-mcp: {error}");
            (None, Some(error.to_string()))
        }
    };

    // Display name for this process's sessions in reports (ADR-0137 clause 2,
    // mirroring `mindleak-mcp`/`lodestar-mcp`'s own `MINDLEAK_AGENT`/
    // `LODESTAR_AGENT`). Since ADR-0054 it is no part of the agent id, so two
    // processes may disagree about it without forking the identity of a
    // session they both host.
    let display_name = std::env::var("ACKPLANE_MCP_AGENT").unwrap_or_else(|_| "agent".to_string());
    let sessions = SessionRegistry::new(&display_name).map_err(anyhow::Error::msg)?;

    server::run(endpoint, refusal, sessions, environment)?;
    Ok(())
}
