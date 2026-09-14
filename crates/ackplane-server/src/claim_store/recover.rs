use std::time::SystemTime;

use super::{
    outcome_tag, ClaimLeaseOutcome, ClaimLeaseResult, ClaimOwner, ClaimRecoverRequest, ClaimStore,
    ClaimStoreError,
};

impl ClaimStore {
    /// Recover an expired, unparked claim whose owner still matches the
    /// operator's expectation. Only the same owner and node preserve the
    /// original evidence window; a custody transfer starts a new one.
    pub async fn recover(
        &self,
        request: &ClaimRecoverRequest,
        now: SystemTime,
    ) -> Result<ClaimLeaseResult, ClaimStoreError> {
        ClaimOwner {
            owner_id: &request.owner_id,
            node_id: &request.node_id,
        }
        .validate()?;
        if request.reason.trim().is_empty() {
            return Err(ClaimStoreError::MissingReason);
        }
        if request.lease.is_zero() {
            return Err(ClaimStoreError::InvalidLease);
        }
        let expires_at = now + request.lease;
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        let existing = transaction
            .query_opt(
                "SELECT owner_id, branch, claim_started_at, lease_expires_at, claim_lapses, paths, symbols, parked, owner_node_id \
                 FROM delegated_claims WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 FOR UPDATE",
                &[&request.tenant_id, &request.repository_id, &request.task_id],
            )
            .await?;
        let result = match existing {
            Some(row) => {
                let existing_owner: String = row.get(0);
                let existing_branch: String = row.get(1);
                let claim_started_at: SystemTime = row.get(2);
                let previous_expiry: SystemTime = row.get(3);
                let previous_lapses: i64 = row.get(4);
                let existing_paths: Vec<String> = row.get(5);
                let existing_symbols: Vec<String> = row.get(6);
                let parked: bool = row.get(7);
                let claim_lapses = u64::try_from(previous_lapses)
                    .map_err(|_| ClaimStoreError::InvalidLapseCount)?;
                if parked || existing_owner != request.expected_owner || previous_expiry >= now {
                    ClaimLeaseResult {
                        outcome: ClaimLeaseOutcome::Rejected,
                        owner_id: existing_owner,
                        branch: existing_branch,
                        claim_started_at,
                        lease_expires_at: previous_expiry,
                        claim_lapses,
                        paths: existing_paths,
                        symbols: existing_symbols,
                    }
                } else {
                    let same_owner = existing_owner == request.owner_id
                        && row.get::<_, Option<&str>>("owner_node_id") == Some(&request.node_id);
                    let granted_branch = if same_owner {
                        existing_branch
                    } else {
                        request.branch.clone()
                    };
                    let granted_started_at = if same_owner { claim_started_at } else { now };
                    let next_lapses = claim_lapses + 1;
                    transaction
                        .execute(
                            "UPDATE delegated_claims SET owner_id = $4, branch = $5, claim_started_at = $6, \
                             lease_expires_at = $7, claim_lapses = $8, paths = $9, symbols = $10, owner_node_id = $11 \
                             WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
                            &[&request.tenant_id, &request.repository_id, &request.task_id, &request.owner_id,
                              &granted_branch, &granted_started_at, &expires_at, &(next_lapses as i64),
                              &request.paths, &request.symbols, &request.node_id],
                        )
                        .await?;
                    ClaimLeaseResult {
                        outcome: ClaimLeaseOutcome::Granted,
                        owner_id: request.owner_id.clone(),
                        branch: granted_branch,
                        claim_started_at: granted_started_at,
                        lease_expires_at: expires_at,
                        claim_lapses: next_lapses,
                        paths: request.paths.clone(),
                        symbols: request.symbols.clone(),
                    }
                }
            }
            None => ClaimLeaseResult {
                outcome: ClaimLeaseOutcome::Rejected,
                owner_id: request.expected_owner.clone(),
                branch: String::new(),
                claim_started_at: now,
                lease_expires_at: now,
                claim_lapses: 0,
                paths: Vec::new(),
                symbols: Vec::new(),
            },
        };
        transaction
            .execute(
                "INSERT INTO delegated_claim_history (tenant_id, repository_id, task_id, requested_owner_id, \
                 granted_owner_id, outcome, claim_started_at, lease_expires_at, claim_lapses, paths, symbols, requested_node_id) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.task_id,
                    &request.owner_id,
                    &result.owner_id,
                    &outcome_tag(result.outcome),
                    &result.claim_started_at,
                    &result.lease_expires_at,
                    &(result.claim_lapses as i64),
                    &result.paths,
                    &result.symbols,
                    &request.node_id,
                ],
            )
            .await?;
        transaction.commit().await?;
        Ok(result)
    }
}
