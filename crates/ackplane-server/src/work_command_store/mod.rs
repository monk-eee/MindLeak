//! Immutable Industrial Work command requests and receipts (ADR-0125).
//!
//! The authoritative command service must validate authorization before calling
//! this store. It records intent and outcome references only; it does not
//! expose a transport or mutate Work/Claim state.
#![allow(dead_code)]

use tokio_postgres::{Client, NoTls};

use crate::migration_lock;

const MIGRATION: &str = include_str!("../../migrations/0037_work_commands.sql");
const WORK_MIGRATION: &str = include_str!("../../migrations/0028_work.sql");

mod model;
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
        Ok(Self { client })
    }
}

#[cfg(test)]
mod tests;
