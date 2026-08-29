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
mod server;
mod tools;

fn main() -> anyhow::Result<()> {
    let environment = |name: &str| std::env::var(name).ok();

    // Settled once, before serving: which arbiter this front door may reach.
    // A refusal deliberately does not exit -- see `server::run`.
    let (endpoint, refusal) = match endpoint::resolve_endpoint(&environment) {
        Ok(endpoint) => (Some(endpoint), None),
        Err(error) => {
            // stderr, never stdout: stdout is the JSON-RPC channel.
            eprintln!("ackplane-mcp: {error}");
            (None, Some(error.to_string()))
        }
    };

    server::run(endpoint, refusal, environment)?;
    Ok(())
}
