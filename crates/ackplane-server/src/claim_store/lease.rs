//! ADR-0096 clauses 2-3 and 6: the CAS lease-mutation operations —
//! delegate, release, renew, recover.

use std::time::{Duration, SystemTime};

use super::{
    ClaimLeaseOutcome, ClaimLeaseRequest, ClaimLeaseResult, ClaimRecoverRequest, ClaimStore,
    ClaimStoreError,
};

impl ClaimStore {
    pub async fn delegate(
        &self,
        request: &ClaimLeaseRequest,
        now: SystemTime,
    ) -> Result<ClaimLeaseResult, ClaimStoreError> {
        if request.lease.is_zero() {
            return Err(ClaimStoreError::InvalidLease);
        }
        let expires_at = now + request.lease;
        // One connection, checked out once and held until commit (ADR-0143
        // decision 4). The `FOR UPDATE` row lock below is session-scoped, so a
        // connection returned to the pool mid-operation would drop the lock
        // this CAS depends on.
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        // At most two passes. `SELECT ... FOR UPDATE` locks rows, and there is
        // no row to lock until a task has been claimed once -- so the very
        // first claim on a task is the one case the row lock cannot arbitrate,
        // and two concurrent callers both read `None`. The insert below is
        // therefore `ON CONFLICT DO NOTHING`, which turns losing that race into
        // an observable zero rows rather than a duplicate-key error; by the
        // time it returns the winner's row is committed, so the second pass
        // locks it and takes the ordinary existing-row decision. Rows here are
        // only ever holed, never deleted, so a third pass cannot arise.
        let result = loop {
            let existing = transaction
                .query_opt(
                    "SELECT owner_id, branch, claim_started_at, lease_expires_at, claim_lapses, paths, symbols, parked \
                     FROM delegated_claims WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 FOR UPDATE",
                    &[&request.tenant_id, &request.repository_id, &request.task_id],
                )
                .await?;

            match existing {
                Some(row) => {
                    let owner_id: String = row.get(0);
                    let branch: String = row.get(1);
                    let claim_started_at: SystemTime = row.get(2);
                    let previous_expiry: SystemTime = row.get(3);
                    let previous_lapses: i64 = row.get(4);
                    let paths: Vec<String> = row.get(5);
                    let symbols: Vec<String> = row.get(6);
                    let parked: bool = row.get(7);
                    let claim_lapses = u64::try_from(previous_lapses)
                        .map_err(|_| ClaimStoreError::InvalidLapseCount)?;
                    // A parked claim keeps its owner's exclusive hold on this
                    // scope regardless of lease expiry (ADR-0096 clause
                    // completion): it deliberately cleared its lease, which
                    // is not the same fact as having lapsed. Only `answer`
                    // may return it to circulation, never `delegate` -- not
                    // even for the parking owner, matching how the local
                    // `needs_input` status also refuses a second
                    // `claim_task`.
                    if parked {
                        break ClaimLeaseResult {
                            outcome: ClaimLeaseOutcome::Rejected,
                            owner_id,
                            branch,
                            claim_started_at,
                            lease_expires_at: previous_expiry,
                            claim_lapses,
                            paths,
                            symbols,
                        };
                    }
                    if owner_id != request.owner_id && previous_expiry >= now {
                        break ClaimLeaseResult {
                            outcome: ClaimLeaseOutcome::Rejected,
                            owner_id,
                            branch,
                            claim_started_at,
                            lease_expires_at: previous_expiry,
                            claim_lapses,
                            paths,
                            symbols,
                        };
                    }
                    let same_owner = owner_id == request.owner_id;
                    let lapsed = previous_expiry < now;
                    let next_lapses = claim_lapses + u64::from(lapsed);
                    let granted_branch = if same_owner {
                        branch
                    } else {
                        request.branch.clone()
                    };
                    let granted_started_at = if same_owner { claim_started_at } else { now };
                    transaction.execute(
                        "UPDATE delegated_claims SET owner_id = $4, branch = $5, claim_started_at = $6, \
                         lease_expires_at = $7, claim_lapses = $8, paths = $9, symbols = $10 \
                         WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
                        &[&request.tenant_id, &request.repository_id, &request.task_id, &request.owner_id,
                          &granted_branch, &granted_started_at, &expires_at, &(next_lapses as i64),
                          &request.paths, &request.symbols],
                    ).await?;
                    break ClaimLeaseResult {
                        outcome: ClaimLeaseOutcome::Granted,
                        owner_id: request.owner_id.clone(),
                        branch: granted_branch,
                        claim_started_at: granted_started_at,
                        lease_expires_at: expires_at,
                        claim_lapses: next_lapses,
                        paths: request.paths.clone(),
                        symbols: request.symbols.clone(),
                    };
                }
                None => {
                    let inserted = transaction.execute(
                        "INSERT INTO delegated_claims (tenant_id, repository_id, task_id, owner_id, branch, \
                         claim_started_at, lease_expires_at, claim_lapses, paths, symbols) \
                         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
                         ON CONFLICT (tenant_id, repository_id, task_id) DO NOTHING",
                        &[&request.tenant_id, &request.repository_id, &request.task_id, &request.owner_id,
                          &request.branch, &now, &expires_at, &0_i64, &request.paths, &request.symbols],
                    ).await?;
                    if inserted == 1 {
                        break ClaimLeaseResult {
                            outcome: ClaimLeaseOutcome::Granted,
                            owner_id: request.owner_id.clone(),
                            branch: request.branch.clone(),
                            claim_started_at: now,
                            lease_expires_at: expires_at,
                            claim_lapses: 0,
                            paths: request.paths.clone(),
                            symbols: request.symbols.clone(),
                        };
                    }
                    // A competitor created this task first. Its row is committed
                    // now, so the next pass locks it and decides normally.
                }
            }
        };

        transaction.execute(
            "INSERT INTO delegated_claim_history (tenant_id, repository_id, task_id, requested_owner_id, \
             granted_owner_id, outcome, claim_started_at, lease_expires_at, claim_lapses, paths, symbols) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
            &[&request.tenant_id, &request.repository_id, &request.task_id, &request.owner_id,
              &result.owner_id, &outcome_tag(result.outcome), &result.claim_started_at,
              &result.lease_expires_at, &(result.claim_lapses as i64), &result.paths, &result.symbols],
        ).await?;
        transaction.commit().await?;
        Ok(result)
    }

    /// Voluntarily give back a live claim before its lease naturally expires
    /// (ADR-0096 decision 6: holed, not extended). Owner-guarded: only the
    /// exact current `owner_id` may release. Holes the lease immediately
    /// (`lease_expires_at = now`) rather than deleting the row, so the
    /// existing `delegate` CAS grants it to the next caller without waiting
    /// out the original lease. Releasing a claim you do not hold, or one that
    /// has already expired, is a no-op: there is nothing live to give back.
    pub async fn release(
        &self,
        tenant_id: &str,
        repository_id: &str,
        task_id: &str,
        owner_id: &str,
        now: SystemTime,
    ) -> Result<bool, ClaimStoreError> {
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        let changed = transaction
            .execute(
                "UPDATE delegated_claims SET lease_expires_at = $5 \
                 WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 \
                   AND owner_id = $4 AND lease_expires_at >= $5",
                &[&tenant_id, &repository_id, &task_id, &owner_id, &now],
            )
            .await?;
        let released = changed == 1;
        transaction
            .execute(
                "INSERT INTO delegated_claim_history (tenant_id, repository_id, task_id, requested_owner_id, \
                 granted_owner_id, outcome, claim_started_at, lease_expires_at, claim_lapses, paths, symbols) \
                 VALUES ($1,$2,$3,$4,$4,$5,$6,$6,0,ARRAY[]::text[],ARRAY[]::text[])",
                &[
                    &tenant_id,
                    &repository_id,
                    &task_id,
                    &owner_id,
                    &release_outcome_tag(released),
                    &now,
                ],
            )
            .await?;
        transaction.commit().await?;
        Ok(released)
    }

    /// Extend a still-live lease the exact current owner holds (ADR-0096
    /// clauses 2-3, matching `lodestar-core`'s `renew_lease`). Does not reset
    /// `claim_started_at`, `branch`, `paths`, or `symbols` -- only
    /// `lease_expires_at` moves. A renew from a non-owner, against a lease
    /// that already expired, or against a task never claimed here, is
    /// rejected: an expired lease needs a fresh `delegate`, not a renewal.
    pub async fn renew(
        &self,
        tenant_id: &str,
        repository_id: &str,
        task_id: &str,
        owner_id: &str,
        lease: Duration,
        now: SystemTime,
    ) -> Result<ClaimLeaseResult, ClaimStoreError> {
        if lease.is_zero() {
            return Err(ClaimStoreError::InvalidLease);
        }
        let expires_at = now + lease;
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        let existing = transaction
            .query_opt(
                "SELECT owner_id, branch, claim_started_at, lease_expires_at, claim_lapses, paths, symbols \
                 FROM delegated_claims WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 FOR UPDATE",
                &[&tenant_id, &repository_id, &task_id],
            )
            .await?;

        let result = match existing {
            Some(row) => {
                let existing_owner: String = row.get(0);
                let branch: String = row.get(1);
                let claim_started_at: SystemTime = row.get(2);
                let previous_expiry: SystemTime = row.get(3);
                let previous_lapses: i64 = row.get(4);
                let paths: Vec<String> = row.get(5);
                let symbols: Vec<String> = row.get(6);
                let claim_lapses = u64::try_from(previous_lapses)
                    .map_err(|_| ClaimStoreError::InvalidLapseCount)?;
                if existing_owner != owner_id || previous_expiry < now {
                    ClaimLeaseResult {
                        outcome: ClaimLeaseOutcome::Rejected,
                        owner_id: existing_owner,
                        branch,
                        claim_started_at,
                        lease_expires_at: previous_expiry,
                        claim_lapses,
                        paths,
                        symbols,
                    }
                } else {
                    transaction
                        .execute(
                            "UPDATE delegated_claims SET lease_expires_at = $4 \
                             WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
                            &[&tenant_id, &repository_id, &task_id, &expires_at],
                        )
                        .await?;
                    ClaimLeaseResult {
                        outcome: ClaimLeaseOutcome::Granted,
                        owner_id: existing_owner,
                        branch,
                        claim_started_at,
                        lease_expires_at: expires_at,
                        claim_lapses,
                        paths,
                        symbols,
                    }
                }
            }
            None => ClaimLeaseResult {
                outcome: ClaimLeaseOutcome::Rejected,
                owner_id: owner_id.to_owned(),
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
                 granted_owner_id, outcome, claim_started_at, lease_expires_at, claim_lapses, paths, symbols) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
                &[
                    &tenant_id,
                    &repository_id,
                    &task_id,
                    &owner_id,
                    &result.owner_id,
                    &outcome_tag(result.outcome),
                    &result.claim_started_at,
                    &result.lease_expires_at,
                    &(result.claim_lapses as i64),
                    &result.paths,
                    &result.symbols,
                ],
            )
            .await?;
        transaction.commit().await?;
        Ok(result)
    }

    /// Take over a claim the caller believes is stranded (ADR-0096 clauses 3
    /// and 6, matching `lodestar-core`'s `recover_claim`). `expected_owner`
    /// must match the row's actual current owner -- a mismatch means the
    /// owner changed concurrently and is rejected rather than blindly
    /// overwritten, which `delegate` does not check. `reason` is required and
    /// travels into the history for audit. Recovery only succeeds once the
    /// lease has genuinely expired: a live lease is never recoverable out
    /// from under its holder (ADR-0096 clause 6, "holed, not extended"). A
    /// same-owner recovery preserves `claim_started_at` and `branch`, exactly
    /// as `delegate`'s same-owner reclaim does (ADR-0048); a different owner
    /// resets both and records the lapse.
    pub async fn recover(
        &self,
        request: &ClaimRecoverRequest,
        now: SystemTime,
    ) -> Result<ClaimLeaseResult, ClaimStoreError> {
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
                "SELECT owner_id, branch, claim_started_at, lease_expires_at, claim_lapses, paths, symbols, parked \
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
                // A parked claim is not a recoverable one (same reasoning as
                // `delegate`): its owner deliberately cleared the lease
                // pending an answer, which `answer` alone may resolve.
                // Recovering it would let a different agent take over a
                // task its rightful owner is mid-question on.
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
                    let same_owner = existing_owner == request.owner_id;
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
                             lease_expires_at = $7, claim_lapses = $8, paths = $9, symbols = $10 \
                             WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
                            &[&request.tenant_id, &request.repository_id, &request.task_id, &request.owner_id,
                              &granted_branch, &granted_started_at, &expires_at, &(next_lapses as i64),
                              &request.paths, &request.symbols],
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
                 granted_owner_id, outcome, claim_started_at, lease_expires_at, claim_lapses, paths, symbols) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
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
                ],
            )
            .await?;
        transaction.commit().await?;
        Ok(result)
    }
}

fn outcome_tag(outcome: ClaimLeaseOutcome) -> i16 {
    match outcome {
        ClaimLeaseOutcome::Granted => 1,
        ClaimLeaseOutcome::Rejected => 2,
    }
}

fn release_outcome_tag(released: bool) -> i16 {
    if released {
        3
    } else {
        4
    }
}
