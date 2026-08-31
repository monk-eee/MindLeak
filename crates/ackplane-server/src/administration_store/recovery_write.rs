//! Persistence for ADR-0119 decision 6's Recovery inspection reports and
//! ADR-0145 decision 1-2's Recovery rehearsal reports.

use std::time::SystemTime;

use super::recovery_model::{
    assigned_inspection_id, assigned_rehearsal_id, from_rehearsal_row, from_row, validate,
    validate_rehearsal, NewRecoveryInspection, NewRecoveryRehearsal,
};
use super::{AdministrationStore, AdministrationStoreError};

pub use super::recovery_model::{RecoveryInspection, RecoveryRehearsal};

impl AdministrationStore {
    /// Records a new inspection report. Requires the named Snapshot request
    /// to already exist (the foreign key enforces this), but takes no
    /// position on whether it succeeded -- inspecting a failed or refused
    /// Snapshot request is meaningless in practice (there is no artifact),
    /// and the caller (the Bridge route) is expected to have already checked
    /// that before running the inspection this records.
    pub async fn record_recovery_inspection(
        &self,
        inspection: &NewRecoveryInspection,
        now: SystemTime,
    ) -> Result<RecoveryInspection, AdministrationStoreError> {
        validate(inspection, now)?;
        let inspection_id = assigned_inspection_id(inspection)?;
        let connection = self.connection().await?;
        let row = connection
            .query_one(
                "INSERT INTO administration_recovery_inspections (inspection_id, request_id, \
                     requested_by, integrity_verified, decryption_verified, archive_valid, \
                     archive_entry_count, reason, occurred_at, recorded_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING *",
                &[
                    &inspection_id,
                    &inspection.request_id,
                    &inspection.requested_by,
                    &inspection.integrity_verified,
                    &inspection.decryption_verified,
                    &inspection.archive_valid,
                    &inspection.archive_entry_count,
                    &inspection.reason,
                    &inspection.occurred_at,
                    &now,
                ],
            )
            .await?;
        from_row(&row)
    }

    /// The most recently recorded inspection for a request, if any.
    pub async fn latest_recovery_inspection(
        &self,
        request_id: &str,
    ) -> Result<Option<RecoveryInspection>, AdministrationStoreError> {
        let connection = self.connection().await?;
        let row = connection
            .query_opt(
                "SELECT * FROM administration_recovery_inspections \
                 WHERE request_id = $1 ORDER BY recorded_at DESC LIMIT 1",
                &[&request_id],
            )
            .await?;
        row.as_ref().map(from_row).transpose()
    }

    /// Records a new rehearsal report (ADR-0145 decision 1-2). Requires the
    /// named Snapshot request to already exist (the foreign key enforces
    /// this), exactly like `record_recovery_inspection`.
    pub async fn record_recovery_rehearsal(
        &self,
        rehearsal: &NewRecoveryRehearsal,
        now: SystemTime,
    ) -> Result<RecoveryRehearsal, AdministrationStoreError> {
        validate_rehearsal(rehearsal, now)?;
        let rehearsal_id = assigned_rehearsal_id(rehearsal)?;
        let connection = self.connection().await?;
        let row = connection
            .query_one(
                "INSERT INTO administration_recovery_rehearsals (rehearsal_id, request_id, \
                     requested_by, manifest_digest, restore_duration_ms, \
                     migration_version_matched, archive_table_count, restored_table_count, \
                     restored_row_count, passed, reason, occurred_at, recorded_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) RETURNING *",
                &[
                    &rehearsal_id,
                    &rehearsal.request_id,
                    &rehearsal.requested_by,
                    &rehearsal.manifest_digest,
                    &rehearsal.restore_duration_ms,
                    &rehearsal.migration_version_matched,
                    &rehearsal.archive_table_count,
                    &rehearsal.restored_table_count,
                    &rehearsal.restored_row_count,
                    &rehearsal.passed,
                    &rehearsal.reason,
                    &rehearsal.occurred_at,
                    &now,
                ],
            )
            .await?;
        from_rehearsal_row(&row)
    }

    /// The most recently recorded *passing* rehearsal for an exact artifact
    /// digest, if any -- what ADR-0145 decision 3's freshness gate looks up.
    /// Scoped to `passed = true` and the exact digest: a passing rehearsal of
    /// a different artifact, or a failed rehearsal of this one, must not
    /// satisfy a later execution's freshness requirement.
    pub async fn latest_passing_recovery_rehearsal(
        &self,
        manifest_digest: &[u8],
    ) -> Result<Option<RecoveryRehearsal>, AdministrationStoreError> {
        let connection = self.connection().await?;
        let row = connection
            .query_opt(
                "SELECT * FROM administration_recovery_rehearsals \
                 WHERE manifest_digest = $1 AND passed = true \
                 ORDER BY recorded_at DESC LIMIT 1",
                &[&manifest_digest],
            )
            .await?;
        row.as_ref().map(from_rehearsal_row).transpose()
    }

    /// One rehearsal report by its own id, so a recovery-execution preview
    /// (`recovery_execution_write::preview_recovery_execution`) can validate
    /// that the caller's *named* rehearsal both passed and covers the exact
    /// artifact digest being restored -- not merely that *some* passing
    /// rehearsal of that digest exists, which `latest_passing_recovery_rehearsal`
    /// alone would not distinguish from a caller naming an unrelated report.
    pub async fn recovery_rehearsal(
        &self,
        rehearsal_id: &str,
    ) -> Result<Option<RecoveryRehearsal>, AdministrationStoreError> {
        let connection = self.connection().await?;
        let row = connection
            .query_opt(
                "SELECT * FROM administration_recovery_rehearsals WHERE rehearsal_id = $1",
                &[&rehearsal_id],
            )
            .await?;
        row.as_ref().map(from_rehearsal_row).transpose()
    }
}
