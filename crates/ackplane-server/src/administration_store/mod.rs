//! ADR-0119's adopted-policy, Snapshot, and Lifecycle-purge request/receipt
//! persistence, and ADR-0128's recognition of the hardened loopback profile
//! as their verified principal.
//!
//! This store never executes a snapshot itself: [`crate::snapshot_provider`]
//! is the one place that shells out to `pg_dump` and encrypts the artifact.
//! This module only ever records the immutable request and its receipt, the
//! same separation `work_command_store` keeps between authorization and
//! delivery. Lifecycle purge is the one exception: its "delivery" is a
//! single scoped, parameterized `DELETE` this store issues itself
//! (`purge_write::delete_purge_candidates`), because unlike a `pg_dump`
//! subprocess a bounded SQL delete against one closed data category needs no
//! separate provider.
#![allow(dead_code)]

use tokio_postgres::{Client, NoTls};

use crate::migration_lock;

const MIGRATION: &str = include_str!("../../migrations/0041_administration.sql");
const PURGE_MIGRATION: &str = include_str!("../../migrations/0042_administration_purge.sql");

mod model;
mod purge_model;
mod purge_write;
mod write;

pub use model::{
    AdministrationOperation, AdministrationPolicy, AdministrationScope, AdministrationStoreError,
    NewSnapshotReceipt, NewSnapshotRequest, PolicyAdoptionRequest, PolicyWriteOutcome,
    SnapshotOutcome, SnapshotReceipt, SnapshotRequest, SnapshotRequestOutcome,
};
pub use purge_model::{
    NewPurgeReceipt, PurgeDataCategory, PurgeOutcome, PurgePreviewRequest, PurgeReceipt,
    PurgeRequest, PurgeRequestOutcome, MAX_CONFIRMATION_WINDOW,
};

/// PostgreSQL persistence for adopted administration policies, Snapshot
/// requests/receipts, and Lifecycle-purge previews/receipts.
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
        migration_lock::migrate_locked(
            &mut client,
            migration_lock::key::ADMINISTRATION_PURGE,
            PURGE_MIGRATION,
        )
        .await?;
        Ok(Self { client })
    }
}

#[cfg(test)]
mod purge_tests;
#[cfg(test)]
mod tests;
