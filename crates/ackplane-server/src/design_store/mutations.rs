//! Design mutations: propose a design and record a lifecycle decision.
//! Split from mod.rs to stay under the module-length ratchet -- the read
//! side already lives in listing.rs.

use super::*;

impl DesignStore {
    /// Propose a design: an idempotent-checked insert of the design row plus
    /// its first (`Proposed`) decision-history row, in one transaction. An
    /// identical retry (same identity fields) is a no-op; the same
    /// `design_id` reused with different content is refused.
    pub async fn create_design(
        &self,
        request: CreateDesignRequest,
    ) -> Result<(), DesignStoreError> {
        if request.design_id.trim().is_empty() {
            return Err(DesignStoreError::EmptyDesignId);
        }
        if request.title.trim().is_empty() {
            return Err(DesignStoreError::EmptyTitle);
        }
        if request.source_version.trim().is_empty() {
            return Err(DesignStoreError::EmptySourceVersion);
        }
        if request.proposed_by.trim().is_empty() {
            return Err(DesignStoreError::EmptyActor);
        }

        let payload = DesignIdentityPayload {
            title: request.title.clone(),
            summary: request.summary.clone(),
            source_version: request.source_version.clone(),
            constitution_version_id: request.constitution_version_id.clone(),
            work_task_id: request.work_task_id.clone(),
            evidence_id: request.evidence_id.clone(),
            display_label: request.display_label.clone(),
        };
        let payload_bytes =
            serde_json::to_vec(&payload).expect("DesignIdentityPayload always serializes");
        let content_digest = Sha256::digest(&payload_bytes).to_vec();

        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        let existing = transaction
            .query_opt(
                "SELECT content_digest FROM industrial_designs \
                 WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.design_id,
                ],
            )
            .await?;
        if let Some(row) = existing {
            let stored_digest: Vec<u8> = row.get(0);
            transaction.commit().await?;
            if stored_digest == content_digest {
                return Ok(());
            }
            return Err(DesignStoreError::DesignImmutabilityViolation {
                design_id: request.design_id.clone(),
            });
        }

        transaction
            .execute(
                "INSERT INTO industrial_designs \
                 (tenant_id, repository_id, design_id, title, summary, source_version, \
                  lifecycle_state, constitution_version_id, work_task_id, evidence_id, \
                  content_digest, display_label) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.design_id,
                    &request.title,
                    &request.summary,
                    &request.source_version,
                    &(DesignLifecycleState::Proposed as i16),
                    &request.constitution_version_id,
                    &request.work_task_id,
                    &request.evidence_id,
                    &content_digest,
                    &request.display_label,
                ],
            )
            .await?;
        transaction
            .execute(
                "INSERT INTO industrial_design_decisions \
                 (tenant_id, repository_id, design_id, sequence_number, decision_kind, actor, \
                  rationale) \
                 VALUES ($1, $2, $3, 1, $4, $5, NULL)",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.design_id,
                    &(DesignLifecycleState::Proposed as i16),
                    &request.proposed_by,
                ],
            )
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Append one decision-history row and move the design's own
    /// `lifecycle_state` to match, in one transaction. The guarded UPDATE
    /// runs first: it only matches a row whose `lifecycle_state` still
    /// equals `expected_lifecycle_state`, so two concurrent decisions
    /// against the same design can never both land -- the loser gets
    /// `LifecycleStateConflict` (an unknown `design_id` gets `UnknownDesign`
    /// instead), mirroring `ClaimStore::recover`'s own compare-and-swap
    /// (ADR-0111). Only once that guarded update actually lands does the
    /// decision-history row get appended. Legality of a particular
    /// transition (e.g. whether `Rejected` may return to `Proposed`) remains
    /// deliberately unenforced beyond that race -- ADR-0121 decision 3
    /// leaves the broader state-machine policy to a later decision.
    pub async fn record_decision(
        &self,
        request: RecordDecisionRequest,
    ) -> Result<(), DesignStoreError> {
        if request.actor.trim().is_empty() {
            return Err(DesignStoreError::EmptyActor);
        }
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        let updated = transaction
            .execute(
                "UPDATE industrial_designs SET lifecycle_state = $4, updated_at = now() \
                 WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3 \
                   AND lifecycle_state = $5",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.design_id,
                    &(request.decision_kind as i16),
                    &(request.expected_lifecycle_state as i16),
                ],
            )
            .await?;
        if updated == 0 {
            let current = transaction
                .query_opt(
                    "SELECT lifecycle_state FROM industrial_designs \
                     WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3",
                    &[
                        &request.tenant_id,
                        &request.repository_id,
                        &request.design_id,
                    ],
                )
                .await?;
            transaction.commit().await?;
            return match current {
                None => Err(DesignStoreError::UnknownDesign {
                    design_id: request.design_id,
                }),
                Some(row) => {
                    let actual_value: i16 = row.get(0);
                    let actual = DesignLifecycleState::try_from(actual_value).map_err(|()| {
                        DesignStoreError::CorruptLifecycleState {
                            design_id: request.design_id.clone(),
                            value: actual_value,
                        }
                    })?;
                    Err(DesignStoreError::LifecycleStateConflict {
                        design_id: request.design_id,
                        expected: request.expected_lifecycle_state,
                        actual,
                    })
                }
            };
        }
        let next_sequence: i64 = transaction
            .query_one(
                "SELECT COALESCE(MAX(sequence_number), 0) + 1 FROM industrial_design_decisions \
                 WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.design_id,
                ],
            )
            .await?
            .get(0);
        transaction
            .execute(
                "INSERT INTO industrial_design_decisions \
                 (tenant_id, repository_id, design_id, sequence_number, decision_kind, actor, \
                  rationale) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.design_id,
                    &next_sequence,
                    &(request.decision_kind as i16),
                    &request.actor,
                    &request.rationale,
                ],
            )
            .await?;
        transaction.commit().await?;
        Ok(())
    }
}
