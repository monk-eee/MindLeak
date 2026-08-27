//! Immutable Industrial Work command requests and receipts (ADR-0125).
//!
//! The authoritative command service must validate authorization before calling
//! this store. It records intent and outcome references, executes the five
//! server-owned commands' Work/Claim effects, and issues the five
//! supervisor-directed commands' ADR-0107 directives -- but exposes no
//! transport of its own; wiring any of this to a Bridge route or the live
//! NodeSync ingestion path remains a later slice (ADR-0125 decision 11).
#![allow(dead_code)]

use tokio_postgres::{Client, NoTls};

use crate::migration_lock;

const MIGRATION: &str = include_str!("../../migrations/0037_work_commands.sql");
const WORK_MIGRATION: &str = include_str!("../../migrations/0028_work.sql");
const EXECUTION_MIGRATION: &str =
    include_str!("../../migrations/0039_work_task_command_execution.sql");
const DIRECTIVES_MIGRATION: &str =
    include_str!("../../migrations/0053_work_command_directives.sql");
const SUPERVISOR_SESSION_MIGRATION: &str =
    include_str!("../../migrations/0024_supervisor_session_projection.sql");
const AGENT_DIRECTIVES_MIGRATION: &str = include_str!("../../migrations/0030_directives.sql");

mod execute;
mod model;
mod payload;
mod service;
mod write;

use model::{
    NewWorkCommand, NewWorkCommandReceipt, WorkCommand, WorkCommandReceipt,
    WorkCommandReceiptWriteOutcome, WorkCommandStoreError, WorkCommandWriteOutcome,
};

/// PostgreSQL persistence for immutable Work command requests and receipts.
pub struct WorkCommandStore {
    pub(crate) client: Client,
}

impl WorkCommandStore {
    pub async fn connect(database_url: &str) -> Result<Self, tokio_postgres::Error> {
        let (mut client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!(%error, "ackplane Work command store connection closed with an error");
            }
        });
        migration_lock::migrate_locked(&mut client, migration_lock::key::WORK, WORK_MIGRATION)
            .await?;
        migration_lock::migrate_locked(&mut client, migration_lock::key::WORK_COMMANDS, MIGRATION)
            .await?;
        migration_lock::migrate_locked(
            &mut client,
            migration_lock::key::WORK_TASK_COMMAND_EXECUTION,
            EXECUTION_MIGRATION,
        )
        .await?;
        // Supervisor-directed commands (Assign/Steer/Pause/Resume/Drain)
        // issue an ADR-0107 directive on this same connection's transaction
        // (see `execute::supervisor_directives`), so this store also owns
        // migrating the directive tables it writes to -- exactly the same
        // dual-migration pattern `directive_store::DirectiveStore::connect`
        // already uses for its own supervisor-session dependency.
        migration_lock::migrate_locked(
            &mut client,
            migration_lock::key::SUPERVISOR_SESSION_PROJECTION,
            SUPERVISOR_SESSION_MIGRATION,
        )
        .await?;
        migration_lock::migrate_locked(
            &mut client,
            migration_lock::key::DIRECTIVES,
            AGENT_DIRECTIVES_MIGRATION,
        )
        .await?;
        migration_lock::migrate_locked(
            &mut client,
            migration_lock::key::WORK_COMMAND_DIRECTIVES,
            DIRECTIVES_MIGRATION,
        )
        .await?;
        Ok(Self { client })
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod supervisor_directive_tests;
