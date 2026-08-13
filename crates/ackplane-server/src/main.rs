//! The Ackplane federation service entry point (ADR-0082).

use std::process::ExitCode;

use ackplane_server::ServerConfig;

fn main() -> ExitCode {
    match ServerConfig::resolve(|key| std::env::var(key).ok()) {
        Ok(config) => {
            println!("{}", config.banner());
            // The ledger schema and its append transaction exist
            // (`ackplane_server::ledger`, ADR-0086), but nothing here accepts a
            // network connection yet: that is ADR-0083's gRPC node protocol, a
            // separate decision. Serving without it would be the dual
            // authority ADR-0082 clause 3 refuses.
            println!(
                "ackplane-server: ledger schema ready; no gRPC service yet, so exiting rather \
                 than accepting work over a protocol that does not exist"
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("ackplane-server: {error}");
            ExitCode::FAILURE
        }
    }
}
