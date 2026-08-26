//! Persistence for ADR-0119 decision 6's Recovery inspection reports.

use std::time::SystemTime;

use super::recovery_model::{assigned_inspection_id, from_row, validate, NewRecoveryInspection};
use super::{AdministrationStore, AdministrationStoreError};

pub use super::recovery_model::RecoveryInspection;

impl AdministrationStore {
    /// Records a new inspection report. Requires the named Snapshot request
    /// to already exist (the foreign key enforces this), but takes no
    /// position on whether it succeeded -- inspecting a failed or refused
    /// Snapshot request is meaningless in practice (there is no artifact),
    /// and the caller (the Bridge route) is expected to have already checked
    /// that before running the inspection this records.
    pub async fn record_recovery_inspection(
        &mut self,
        inspection: &NewRecoveryInspection,
        now: SystemTime,
    ) -> Result<RecoveryInspection, AdministrationStoreError> {
        validate(inspection, now)?;
        let inspection_id = assigned_inspection_id(inspection)?;
        let row = self
            .client
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
        &mut self,
        request_id: &str,
    ) -> Result<Option<RecoveryInspection>, AdministrationStoreError> {
        let row = self
            .client
            .query_opt(
                "SELECT * FROM administration_recovery_inspections \
                 WHERE request_id = $1 ORDER BY recorded_at DESC LIMIT 1",
                &[&request_id],
            )
            .await?;
        row.as_ref().map(from_row).transpose()
    }
}
