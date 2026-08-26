//! ADR-0119's adopted-policy, Snapshot, Lifecycle-purge, and Export
//! request/receipt persistence, and ADR-0128's recognition of the hardened
//! loopback profile as their verified principal.
//!
//! This store never executes a snapshot itself: [`crate::snapshot_provider`]
//! is the one place that shells out to `pg_dump` and encrypts the artifact.
//! This module only ever records the immutable request and its receipt, the
//! same separation `work_command_store` keeps between authorization and
//! delivery. Lifecycle purge is the one exception: its "delivery" is a
//! single scoped, parameterized `DELETE` this store issues itself
//! (`purge_write::delete_purge_candidates`), because unlike a `pg_dump`
//! subprocess a bounded SQL delete against one closed data category needs no
//! separate provider. Export is built by [`crate::export_provider`], the
//! same separation as Snapshot.
#![allow(dead_code)]

use tokio_postgres::{Client, NoTls};

use crate::migration_lock;

const MIGRATION: &str = include_str!("../../migrations/0041_administration.sql");
const PURGE_MIGRATION: &str = include_str!("../../migrations/0042_administration_purge.sql");
const RECOVERY_INSPECTION_MIGRATION: &str =
    include_str!("../../migrations/0046_administration_recovery_inspection.sql");
const EXPORT_MIGRATION: &str = include_str!("../../migrations/0047_administration_export.sql");
const PURGE_CONFIRMING_LABEL_MIGRATION: &str =
    include_str!("../../migrations/0050_administration_purge_confirming_label.sql");

mod export_model;
mod export_write;
mod model;
mod purge_model;
mod purge_write;
mod recovery_model;
mod recovery_write;
mod write;

pub use export_model::{
    ExportDataCategory, ExportOutcome, ExportReceipt, ExportRequest, ExportRequestOutcome,
    NewExportReceipt, NewExportRequest, MAX_EXPORT_RECORDS,
};
pub use model::{
    AdministrationOperation, AdministrationPolicy, AdministrationScope, AdministrationStoreError,
    NewSnapshotReceipt, NewSnapshotRequest, PolicyAdoptionRequest, PolicyWriteOutcome,
    SnapshotOutcome, SnapshotReceipt, SnapshotRequest, SnapshotRequestOutcome,
};
pub use purge_model::{
    NewPurgeReceipt, PurgeDataCategory, PurgeOutcome, PurgePreviewRequest, PurgeReceipt,
    PurgeRequest, PurgeRequestOutcome, MAX_CONFIRMATION_WINDOW,
};
pub use recovery_model::NewRecoveryInspection;
pub use recovery_write::RecoveryInspection;

/// PostgreSQL persistence for adopted administration policies, Snapshot
/// requests/receipts, Lifecycle-purge previews/receipts, Recovery inspection
/// reports, and Export requests/receipts.
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
        migration_lock::migrate_locked(
            &mut client,
            migration_lock::key::ADMINISTRATION_RECOVERY_INSPECTION,
            RECOVERY_INSPECTION_MIGRATION,
        )
        .await?;
        migration_lock::migrate_locked(
            &mut client,
            migration_lock::key::ADMINISTRATION_EXPORT,
            EXPORT_MIGRATION,
        )
        .await?;
        migration_lock::migrate_locked(
            &mut client,
            migration_lock::key::ADMINISTRATION_PURGE_CONFIRMING_LABEL,
            PURGE_CONFIRMING_LABEL_MIGRATION,
        )
        .await?;
        Ok(Self { client })
    }
}

#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod purge_tests;
#[cfg(test)]
mod recovery_tests;
#[cfg(test)]
mod tests;
