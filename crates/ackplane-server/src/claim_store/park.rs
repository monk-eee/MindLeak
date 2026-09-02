//! ADR-0096 clause completion: the CAS lease-mutation operations for a
//! parked claim -- park, answer -- split out of `lease.rs` to keep it under
//! the 450-line module-length ratchet.

use std::time::{Duration, SystemTime};

use super::{ClaimLeaseOutcome, ClaimLeaseResult, ClaimStore, ClaimStoreError};

impl ClaimStore {
    /// Park a claimed task: owner-guarded transition that keeps the owner
    /// and `claim_started_at` evidence window (ADR-0096 clause completion,
    /// mirroring the local `needs_input` transition, ADR-0020) but clears
    /// the live lease and sets `parked`, which excludes it from `delegate`
    /// and `recover` until `answer` returns it to circulation. A park from
    /// a non-owner, or against an already-parked or never-claimed task, is
    /// refused: parking is not idempotent the way a release is, because a
    /// second park could silently overwrite who is actually waiting on the
    /// answer.
    pub async fn park(
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
                "UPDATE delegated_claims SET lease_expires_at = $5, parked = true \
                 WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 \
                   AND owner_id = $4 AND NOT parked",
                &[&tenant_id, &repository_id, &task_id, &owner_id, &now],
            )
            .await?;
        let parked = changed == 1;
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
                    &park_outcome_tag(parked),
                    &now,
                ],
            )
            .await?;
        transaction.commit().await?;
        Ok(parked)
    }

    /// Answer a parked task: the counterpart to `park`, owner-guarded and
    /// `parked`-guarded (ADR-0096 clause completion, mirroring the local
    /// `answer_question` transition, ADR-0020). Grants a fresh lease to the
    /// exact parking owner -- never a different one, unlike `delegate`/
    /// `recover` -- and clears `parked`. An answer from a non-owner, or
    /// against a task that is not currently parked, is rejected.
    pub async fn answer(
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
                "SELECT owner_id, branch, claim_started_at, lease_expires_at, claim_lapses, paths, symbols, parked \
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
                let parked: bool = row.get(7);
                let claim_lapses = u64::try_from(previous_lapses)
                    .map_err(|_| ClaimStoreError::InvalidLapseCount)?;
                if existing_owner != owner_id || !parked {
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
                            "UPDATE delegated_claims SET lease_expires_at = $4, parked = false \
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
                    &answer_outcome_tag(result.outcome),
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

fn park_outcome_tag(parked: bool) -> i16 {
    if parked {
        5
    } else {
        6
    }
}

fn answer_outcome_tag(outcome: ClaimLeaseOutcome) -> i16 {
    match outcome {
        ClaimLeaseOutcome::Granted => 7,
        ClaimLeaseOutcome::Rejected => 8,
    }
}
