//! Durable, Ackplane-authoritative delegated task claim leases.

use std::time::{Duration, SystemTime};

use thiserror::Error;
use tokio_postgres::{Client, NoTls};

const MIGRATION: &str = include_str!("../migrations/0005_claim_delegation.sql");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimLeaseRequest {
    pub tenant_id: String,
    pub repository_id: String,
    pub task_id: String,
    pub owner_id: String,
    pub branch: String,
    pub lease: Duration,
    pub paths: Vec<String>,
    pub symbols: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimLeaseOutcome {
    Granted,
    Rejected,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimLeaseResult {
    pub outcome: ClaimLeaseOutcome,
    pub owner_id: String,
    pub branch: String,
    pub claim_started_at: SystemTime,
    pub lease_expires_at: SystemTime,
    pub claim_lapses: u64,
    pub paths: Vec<String>,
    pub symbols: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ClaimStoreError {
    #[error("claim delegation database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("lease duration must be greater than zero")]
    InvalidLease,
    #[error("claim_lapses cannot be negative")]
    InvalidLapseCount,
}

pub struct ClaimStore {
    client: Client,
}

impl ClaimStore {
    pub async fn connect(database_url: &str) -> Result<Self, tokio_postgres::Error> {
        let (client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!(%error, "ackplane claim delegation connection closed with an error");
            }
        });
        client.batch_execute(MIGRATION).await?;
        Ok(Self { client })
    }

    pub async fn delegate(
        &mut self,
        request: &ClaimLeaseRequest,
        now: SystemTime,
    ) -> Result<ClaimLeaseResult, ClaimStoreError> {
        if request.lease.is_zero() {
            return Err(ClaimStoreError::InvalidLease);
        }
        let expires_at = now + request.lease;
        let transaction = self.client.transaction().await?;
        let existing = transaction
            .query_opt(
                "SELECT owner_id, branch, claim_started_at, lease_expires_at, claim_lapses, paths, symbols \
                 FROM delegated_claims WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 FOR UPDATE",
                &[&request.tenant_id, &request.repository_id, &request.task_id],
            )
            .await?;

        let result = match existing {
            Some(row) => {
                let owner_id: String = row.get(0);
                let branch: String = row.get(1);
                let claim_started_at: SystemTime = row.get(2);
                let previous_expiry: SystemTime = row.get(3);
                let previous_lapses: i64 = row.get(4);
                let paths: Vec<String> = row.get(5);
                let symbols: Vec<String> = row.get(6);
                let claim_lapses = u64::try_from(previous_lapses)
                    .map_err(|_| ClaimStoreError::InvalidLapseCount)?;
                if owner_id != request.owner_id && previous_expiry >= now {
                    ClaimLeaseResult {
                        outcome: ClaimLeaseOutcome::Rejected,
                        owner_id,
                        branch,
                        claim_started_at,
                        lease_expires_at: previous_expiry,
                        claim_lapses,
                        paths,
                        symbols,
                    }
                } else {
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
            None => {
                transaction.execute(
                    "INSERT INTO delegated_claims (tenant_id, repository_id, task_id, owner_id, branch, \
                     claim_started_at, lease_expires_at, claim_lapses, paths, symbols) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
                    &[&request.tenant_id, &request.repository_id, &request.task_id, &request.owner_id,
                      &request.branch, &now, &expires_at, &0_i64, &request.paths, &request.symbols],
                ).await?;
                ClaimLeaseResult {
                    outcome: ClaimLeaseOutcome::Granted,
                    owner_id: request.owner_id.clone(),
                    branch: request.branch.clone(),
                    claim_started_at: now,
                    lease_expires_at: expires_at,
                    claim_lapses: 0,
                    paths: request.paths.clone(),
                    symbols: request.symbols.clone(),
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
        &mut self,
        tenant_id: &str,
        repository_id: &str,
        task_id: &str,
        owner_id: &str,
        now: SystemTime,
    ) -> Result<bool, ClaimStoreError> {
        let transaction = self.client.transaction().await?;
        let changed = transaction
            .execute(
                "UPDATE delegated_claims SET lease_expires_at = $5 \
                 WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 \
                   AND owner_id = $4 AND lease_expires_at > $5",
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
        &mut self,
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
        let transaction = self.client.transaction().await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn request(tenant_id: &str, task_id: &str, owner_id: &str) -> ClaimLeaseRequest {
        ClaimLeaseRequest {
            tenant_id: tenant_id.to_owned(),
            repository_id: "repository".to_owned(),
            task_id: task_id.to_owned(),
            owner_id: owner_id.to_owned(),
            branch: format!("branch/{owner_id}"),
            lease: Duration::from_secs(60),
            paths: vec![format!("src/{owner_id}.rs")],
            symbols: vec![format!("symbol:{owner_id}")],
        }
    }

    #[tokio::test]
    async fn authoritative_store_refuses_a_live_competitor_and_allows_expired_reclaim() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-store-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let second = request(&tenant_id, task_id, "owner-two");
        let mut store = ClaimStore::connect(&database_url).await.unwrap();

        let granted = store.delegate(&first, now).await.unwrap();
        assert_eq!(granted.outcome, ClaimLeaseOutcome::Granted);
        assert_eq!(granted.owner_id, "owner-one");
        assert_eq!(granted.paths, first.paths);

        let rejected = store
            .delegate(&second, now + Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(rejected.outcome, ClaimLeaseOutcome::Rejected);
        assert_eq!(rejected.owner_id, "owner-one");
        assert_eq!(rejected.paths, first.paths);

        let reclaimed = store
            .delegate(&second, now + Duration::from_secs(61))
            .await
            .unwrap();
        assert_eq!(reclaimed.outcome, ClaimLeaseOutcome::Granted);
        assert_eq!(reclaimed.owner_id, "owner-two");
        assert_eq!(reclaimed.claim_lapses, 1);
        assert_eq!(reclaimed.paths, second.paths);
    }

    #[tokio::test]
    async fn the_live_owner_can_release_before_expiry_and_a_different_owner_claims_immediately() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-release-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let second = request(&tenant_id, task_id, "owner-two");
        let mut store = ClaimStore::connect(&database_url).await.unwrap();

        store.delegate(&first, now).await.unwrap();

        // Without a release, a different owner is refused while the lease is live.
        let still_live = store
            .delegate(&second, now + Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(still_live.outcome, ClaimLeaseOutcome::Rejected);

        let released = store
            .release(
                &tenant_id,
                "repository",
                task_id,
                "owner-one",
                now + Duration::from_secs(2),
            )
            .await
            .unwrap();
        assert!(released, "the live owner must be able to release");

        // Immediately grantable to a different owner, with no wait for the
        // original 60-second lease to naturally expire.
        let granted_after_release = store
            .delegate(&second, now + Duration::from_secs(3))
            .await
            .unwrap();
        assert_eq!(granted_after_release.outcome, ClaimLeaseOutcome::Granted);
        assert_eq!(granted_after_release.owner_id, "owner-two");
    }

    #[tokio::test]
    async fn a_non_owner_release_is_refused_and_does_not_affect_the_live_claim() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-release-refused-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let mut store = ClaimStore::connect(&database_url).await.unwrap();

        store.delegate(&first, now).await.unwrap();

        let released = store
            .release(
                &tenant_id,
                "repository",
                task_id,
                "owner-two",
                now + Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert!(
            !released,
            "a non-owner must never release someone else's claim"
        );

        // The original owner's lease is untouched: a competitor is still refused.
        let still_rejected = store
            .delegate(
                &request(&tenant_id, task_id, "owner-two"),
                now + Duration::from_secs(2),
            )
            .await
            .unwrap();
        assert_eq!(still_rejected.outcome, ClaimLeaseOutcome::Rejected);
        assert_eq!(still_rejected.owner_id, "owner-one");
    }

    #[tokio::test]
    async fn releasing_an_already_expired_claim_is_a_no_op() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-release-expired-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let mut store = ClaimStore::connect(&database_url).await.unwrap();

        store.delegate(&first, now).await.unwrap();

        // Past the 60-second lease: nothing live remains to release.
        let released = store
            .release(
                &tenant_id,
                "repository",
                task_id,
                "owner-one",
                now + Duration::from_secs(61),
            )
            .await
            .unwrap();
        assert!(!released, "releasing an already-expired lease is a no-op");
    }

    #[tokio::test]
    async fn the_live_owner_can_renew_and_the_started_at_and_scope_are_untouched() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-renew-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let mut store = ClaimStore::connect(&database_url).await.unwrap();

        let granted = store.delegate(&first, now).await.unwrap();

        let renewed = store
            .renew(
                &tenant_id,
                "repository",
                task_id,
                "owner-one",
                Duration::from_secs(120),
                now + Duration::from_secs(30),
            )
            .await
            .unwrap();

        assert_eq!(renewed.outcome, ClaimLeaseOutcome::Granted);
        assert_eq!(renewed.owner_id, "owner-one");
        assert_eq!(
            renewed.claim_started_at, granted.claim_started_at,
            "renew must not reset claim_started_at"
        );
        assert_eq!(
            renewed.branch, granted.branch,
            "renew must not change branch"
        );
        assert_eq!(renewed.paths, granted.paths, "renew must not change scope");
        assert_eq!(
            renewed.lease_expires_at,
            now + Duration::from_secs(30) + Duration::from_secs(120)
        );
        assert!(
            renewed.lease_expires_at > granted.lease_expires_at,
            "renew must extend the lease"
        );
    }

    #[tokio::test]
    async fn a_non_owner_renew_is_refused_and_does_not_affect_the_live_claim() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-renew-refused-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let mut store = ClaimStore::connect(&database_url).await.unwrap();

        let granted = store.delegate(&first, now).await.unwrap();

        let rejected = store
            .renew(
                &tenant_id,
                "repository",
                task_id,
                "owner-two",
                Duration::from_secs(120),
                now + Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert_eq!(rejected.outcome, ClaimLeaseOutcome::Rejected);
        assert_eq!(rejected.owner_id, "owner-one");

        // The original owner's lease is untouched by the refused attempt.
        let still_rejected_competitor = store
            .delegate(
                &request(&tenant_id, task_id, "owner-two"),
                now + Duration::from_secs(2),
            )
            .await
            .unwrap();
        assert_eq!(
            still_rejected_competitor.outcome,
            ClaimLeaseOutcome::Rejected
        );
        assert_eq!(
            still_rejected_competitor.lease_expires_at, granted.lease_expires_at,
            "a refused renew must not have extended the lease"
        );
    }

    #[tokio::test]
    async fn renewing_an_already_expired_claim_is_refused_not_extended() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-renew-expired-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let mut store = ClaimStore::connect(&database_url).await.unwrap();

        store.delegate(&first, now).await.unwrap();

        // Past the 60-second lease: nothing live remains to renew.
        let rejected = store
            .renew(
                &tenant_id,
                "repository",
                task_id,
                "owner-one",
                Duration::from_secs(120),
                now + Duration::from_secs(61),
            )
            .await
            .unwrap();
        assert_eq!(
            rejected.outcome,
            ClaimLeaseOutcome::Rejected,
            "an expired lease needs a fresh claim, not a renewal"
        );
    }
}
