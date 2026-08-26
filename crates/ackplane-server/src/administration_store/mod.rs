//! ADR-0119's adopted-policy and Snapshot request/receipt persistence, and
//! ADR-0128's recognition of the hardened loopback profile as their verified
//! principal.
//!
//! This store never executes a snapshot itself: [`crate::snapshot_provider`]
//! is the one place that shells out to `pg_dump` and encrypts the artifact.
//! This module only ever records the immutable request and its receipt, the
//! same separation `work_command_store` keeps between authorization and
//! delivery.
#![allow(dead_code)]

use tokio_postgres::{Client, NoTls};

use crate::migration_lock;

const MIGRATION: &str = include_str!("../../migrations/0041_administration.sql");

mod model;
mod write;

pub use model::{
    AdministrationOperation, AdministrationPolicy, AdministrationScope, AdministrationStoreError,
    NewSnapshotReceipt, NewSnapshotRequest, PolicyAdoptionRequest, PolicyWriteOutcome,
    SnapshotOutcome, SnapshotReceipt, SnapshotRequest, SnapshotRequestOutcome,
};

/// PostgreSQL persistence for adopted administration policies and Snapshot
/// requests/receipts.
pub struct AdministrationStore {
    pub(crate) client: Client,
}

impl AdministrationStore {
    pub async fn connect(database_url: &str) -> Result<Self, tokio_postgres::Error> {
        let (mut client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!(%error, "ackplane Administration store connection closed with an error");
            }
        });
        migration_lock::migrate_locked(&mut client, migration_lock::key::ADMINISTRATION, MIGRATION)
            .await?;
        Ok(Self { client })
    }
}

#[cfg(test)]
mod tests;
