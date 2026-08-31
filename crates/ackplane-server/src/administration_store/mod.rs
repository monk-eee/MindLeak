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
//! same separation as Snapshot. ADR-0145 decision 4-5's recovery-execution
//! preview/confirmation (`recovery_execution_write`) follows the same rule
//! twice over: the safety Snapshot it requires is triggered by the caller
//! before this store ever sees the preview, and confirming here never runs
//! `pg_restore` -- it only records that a second, distinct enrolled key
//! authorized the request. Decision 7's own execution step
//! (`recovery_execution_receipt_write`) is the one exception left: it is
//! this store's only call into `snapshot_provider::execute_recovery`, the
//! sole place a `pg_restore` runs against the real, authoritative database.
#![allow(dead_code)]

use crate::db_pool::{PgConnection, PgPool};
use crate::migration_lock;

const MIGRATION: &str = include_str!("../../migrations/0041_administration.sql");
const PURGE_MIGRATION: &str = include_str!("../../migrations/0042_administration_purge.sql");
const RECOVERY_INSPECTION_MIGRATION: &str =
    include_str!("../../migrations/0046_administration_recovery_inspection.sql");
const EXPORT_MIGRATION: &str = include_str!("../../migrations/0047_administration_export.sql");
const PURGE_CONFIRMING_LABEL_MIGRATION: &str =
    include_str!("../../migrations/0050_administration_purge_confirming_label.sql");
const PURGE_CONFIRMATION_AUTHENTICATION_MIGRATION: &str =
    include_str!("../../migrations/0051_administration_purge_confirmation_authentication.sql");
const PURGE_CONFIRMATION_FINGERPRINT_MIGRATION: &str =
    include_str!("../../migrations/0052_administration_purge_confirmation_fingerprint.sql");
const RECOVERY_REHEARSAL_MIGRATION: &str =
    include_str!("../../migrations/0057_administration_recovery_rehearsal.sql");
const RECOVERY_EXECUTION_MIGRATION: &str =
    include_str!("../../migrations/0058_administration_recovery_execution.sql");
const RECOVERY_EXECUTION_RECEIPT_MIGRATION: &str =
    include_str!("../../migrations/0063_administration_recovery_execution_receipt.sql");

mod export_model;
mod export_write;
mod model;
mod purge_model;
mod purge_write;
mod recovery_execution_model;
mod recovery_execution_receipt_model;
mod recovery_execution_receipt_write;
mod recovery_execution_write;
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
pub use recovery_execution_model::{
    NewRecoveryConfirmation, RecoveryConfirmation, RecoveryConfirmationOutcome,
    RecoveryExecutionPreviewRequest, RecoveryExecutionRequest, RecoveryExecutionRequestOutcome,
};
pub use recovery_execution_receipt_model::{RecoveryExecutionOutcome, RecoveryExecutionReceipt};
pub use recovery_model::{NewRecoveryInspection, NewRecoveryRehearsal};
pub use recovery_write::{RecoveryInspection, RecoveryRehearsal};

/// PostgreSQL persistence for adopted administration policies, Snapshot
/// requests/receipts, Lifecycle-purge previews/receipts, Recovery inspection
/// reports, and Export requests/receipts.
pub struct AdministrationStore {
    pool: PgPool,
}

impl AdministrationStore {
    /// Takes a clone of the process's single pool (ADR-0143 decision 1), not
    /// a database URL: a store that resolved its own connection would be
    /// exactly the per-store demand the pool exists to bound.
    pub async fn connect(pool: &PgPool) -> Result<Self, AdministrationStoreError> {
        let mut connection = pool.get().await?;
        migration_lock::migrate_locked(
            &mut connection,
            migration_lock::key::ADMINISTRATION,
            MIGRATION,
        )
        .await?;
        migration_lock::migrate_locked(
            &mut connection,
            migration_lock::key::ADMINISTRATION_PURGE,
            PURGE_MIGRATION,
        )
        .await?;
        migration_lock::migrate_locked(
            &mut connection,
            migration_lock::key::ADMINISTRATION_RECOVERY_INSPECTION,
            RECOVERY_INSPECTION_MIGRATION,
        )
        .await?;
        migration_lock::migrate_locked(
            &mut connection,
            migration_lock::key::ADMINISTRATION_EXPORT,
            EXPORT_MIGRATION,
        )
        .await?;
        migration_lock::migrate_locked(
            &mut connection,
            migration_lock::key::ADMINISTRATION_PURGE_CONFIRMING_LABEL,
            PURGE_CONFIRMING_LABEL_MIGRATION,
        )
        .await?;
        migration_lock::migrate_locked(
            &mut connection,
            migration_lock::key::ADMINISTRATION_PURGE_CONFIRMATION_AUTHENTICATION,
            PURGE_CONFIRMATION_AUTHENTICATION_MIGRATION,
        )
        .await?;
        migration_lock::migrate_locked(
            &mut connection,
            migration_lock::key::ADMINISTRATION_PURGE_CONFIRMATION_FINGERPRINT,
            PURGE_CONFIRMATION_FINGERPRINT_MIGRATION,
        )
        .await?;
        migration_lock::migrate_locked(
            &mut connection,
            migration_lock::key::ADMINISTRATION_RECOVERY_REHEARSAL,
            RECOVERY_REHEARSAL_MIGRATION,
        )
        .await?;
        migration_lock::migrate_locked(
            &mut connection,
            migration_lock::key::ADMINISTRATION_RECOVERY_EXECUTION,
            RECOVERY_EXECUTION_MIGRATION,
        )
        .await?;
        migration_lock::migrate_locked(
            &mut connection,
            migration_lock::key::ADMINISTRATION_RECOVERY_EXECUTION_RECEIPT,
            RECOVERY_EXECUTION_RECEIPT_MIGRATION,
        )
        .await?;
        Ok(Self { pool: pool.clone() })
    }

    /// One checked-out connection, held only for the call that asked for it.
    ///
    /// A caller that opens a transaction keeps this binding alive for the
    /// life of that transaction, which is the one case where holding a
    /// connection across `.await` points is correct rather than accidental.
    pub(crate) async fn connection(&self) -> Result<PgConnection, AdministrationStoreError> {
        Ok(self.pool.get().await?)
    }
}

#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod purge_tests;
#[cfg(test)]
mod recovery_execution_tests;
#[cfg(test)]
mod recovery_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod migration_tests {
    use super::{
        PURGE_CONFIRMATION_AUTHENTICATION_MIGRATION, PURGE_CONFIRMATION_FINGERPRINT_MIGRATION,
    };

    // Regression: installations that already recorded key 51 never rerun its
    // SQL, so fingerprint columns must remain in a later immutable migration.
    #[test]
    fn fingerprint_upgrade_does_not_rewrite_the_consumed_authentication_migration() {
        assert!(
            !PURGE_CONFIRMATION_AUTHENTICATION_MIGRATION.contains("public_key_fingerprint"),
            "key 51 may already be applied and must stay immutable"
        );
        assert!(
            PURGE_CONFIRMATION_FINGERPRINT_MIGRATION.contains("public_key_fingerprint"),
            "key 52 must carry the fingerprint upgrade"
        );
    }
}
