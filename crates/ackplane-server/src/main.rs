//! The Ackplane federation service entry point (ADR-0082).

use std::process::ExitCode;

use ackplane_server::ServerConfig;

fn main() -> ExitCode {
    match ServerConfig::resolve(|key| std::env::var(key).ok()) {
        Ok(config) => {
            println!("{}", config.banner());
            // Serving is the next decision's work: there is no ledger to serve
            // from until ADR-0086's schema exists, and a listener that accepted
            // work without one would be the dual authority ADR-0082 clause 3
            // refuses.
            println!(
                "ackplane-server: no ledger implementation yet; exiting rather than \
                 accepting work it cannot durably record"
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("ackplane-server: {error}");
            ExitCode::FAILURE
        }
    }
}
