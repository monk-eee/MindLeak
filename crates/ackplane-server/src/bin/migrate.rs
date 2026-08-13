//! The Ackplane migration entrypoint (ADR-0088 clause 5): a one-shot process
//! that applies every table this deployment needs and exits, so the
//! `ackplane` service can start only after migrations have finished rather
//! than racing them at boot.
//!
//! It applies no migration logic of its own: [`LedgerStore::connect`] and
//! [`Projector::connect`] each run their own idempotent `CREATE TABLE IF NOT
//! EXISTS` migration as a side effect of connecting, so this binary's only
//! job is to open both connections in the Compose topology's `migrate`
//! service and report success or failure.

use std::process::ExitCode;

use ackplane_server::ledger::LedgerStore;
use ackplane_server::projection::Projector;

const DATABASE_URL_ENV: &str = "ACKPLANE_DATABASE_URL";

#[tokio::main]
async fn main() -> ExitCode {
    let Ok(database_url) = std::env::var(DATABASE_URL_ENV) else {
        eprintln!("ackplane-migrate: {DATABASE_URL_ENV} is not set");
        return ExitCode::FAILURE;
    };

    if let Err(error) = LedgerStore::connect(&database_url).await {
        eprintln!("ackplane-migrate: ledger schema failed: {error}");
        return ExitCode::FAILURE;
    }
    if let Err(error) = Projector::connect(&database_url).await {
        eprintln!("ackplane-migrate: projection schema failed: {error}");
        return ExitCode::FAILURE;
    }

    println!("ackplane-migrate: ledger and projection schemas are up to date");
    ExitCode::SUCCESS
}
