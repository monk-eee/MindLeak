//! Durable, Ackplane-authoritative delegated task claim leases.

use std::time::{Duration, SystemTime};

use thiserror::Error;

use crate::db_pool::{PgConnection, PgPool};

mod lease;
mod park;

const MIGRATION: &str = include_str!("../../migrations/0005_claim_delegation.sql");
const NONCE_MIGRATION: &str = include_str!("../../migrations/0006_claim_authentication_nonces.sql");
const PARKED_MIGRATION: &str = include_str!("../../migrations/0066_delegated_claim_parked.sql");

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

/// Everything `ClaimStore::recover` needs, bundled to keep the method's own
/// argument count sane (`delegate` does the same with `ClaimLeaseRequest`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimRecoverRequest {
    pub tenant_id: String,
    pub repository_id: String,
    pub task_id: String,
    pub expected_owner: String,
    pub owner_id: String,
    pub reason: String,
    pub branch: String,
    pub lease: Duration,
    pub paths: Vec<String>,
    pub symbols: Vec<String>,
}

/// One claim whose lease has not yet expired, as `list_active` reports it
/// (ADR-0096 clause 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveClaim {
    pub task_id: String,
    pub owner_id: String,
    pub branch: String,
    pub lease_expires_at: SystemTime,
    pub paths: Vec<String>,
    pub symbols: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ClaimStoreError {
    #[error("claim delegation database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("claim delegation could not obtain a database connection: {0}")]
    PoolExhausted(#[from] deadpool_postgres::PoolError),
    #[error("claim delegation signing key error: {0}")]
    SigningKey(#[from] crate::signing_keys::SigningKeyError),
    #[error("lease duration must be greater than zero")]
    InvalidLease,
    #[error("claim_lapses cannot be negative")]
    InvalidLapseCount,
    #[error("claim recovery requires a reason")]
    MissingReason,
}

pub struct ClaimStore {
    pool: PgPool,
}

impl ClaimStore {
    /// Takes a clone of the process's single pool (ADR-0143 decision 1), not a
    /// database URL: a store that resolved its own connection would be exactly
    /// the per-store demand the pool exists to bound.
    pub async fn connect(pool: &PgPool) -> Result<Self, ClaimStoreError> {
        let mut connection = pool.get().await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::CLAIM_DELEGATION,
            MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::CLAIM_AUTHENTICATION_NONCES,
            NONCE_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::DELEGATED_CLAIM_PARKED,
            PARKED_MIGRATION,
        )
        .await?;
        Ok(Self { pool: pool.clone() })
    }

    async fn connection(&self) -> Result<PgConnection, ClaimStoreError> {
        Ok(self.pool.get().await?)
    }

    /// Resolve the signing key a claim request's authentication claims,
    /// judged as of now. Mirrors `LedgerStore::resolve_signing_key`: the
    /// decision itself lives in `signing_keys` and is pure, this only owns
    /// the connection.
    ///
    /// Returns `ClaimStoreError` rather than `SigningKeyError` because
    /// obtaining the connection is now a way this can fail, and that is the
    /// store's concern -- `signing_keys` never sees a pool.
    pub async fn resolve_signing_key(
        &self,
        binding: &crate::signing_keys::EnvelopeBinding<'_>,
    ) -> Result<crate::signing_keys::KeyResolution, ClaimStoreError> {
        let connection = self.connection().await?;
        Ok(crate::signing_keys::resolve(&connection, binding).await?)
    }

    /// Consume a (signing_key_id, nonce) pair exactly once (anti-replay for
    /// `ClaimDelegationService` authentication --
    /// gaps.d/claim-authentication-can-be-replayed-across-operations.md).
    /// Returns true the first time a pair is seen, false on every later
    /// attempt with the identical pair -- the insert's own uniqueness is the
    /// enforcement, so this needs no read-then-write race.
    pub async fn consume_claim_nonce(
        &self,
        signing_key_id: &str,
        nonce: &[u8],
        now: SystemTime,
    ) -> Result<bool, ClaimStoreError> {
        let inserted = self
            .connection()
            .await?
            .execute(
                "INSERT INTO claim_authentication_nonces (signing_key_id, nonce, consumed_at) \
                 VALUES ($1, $2, $3) ON CONFLICT (signing_key_id, nonce) DO NOTHING",
                &[&signing_key_id, &nonce, &now],
            )
            .await?;
        Ok(inserted == 1)
    }

    /// Every claim in this repository whose lease has not yet expired
    /// (ADR-0096 clause 5) - the federated counterpart to `lodestar-core`'s
    /// local `check_claim_overlap`, which has no way to see a delegated
    /// claim at all. Read-only: unlike `delegate`/`release`/`renew`/`recover`,
    /// this never writes `delegated_claim_history`, because listing a claim
    /// changes nothing about it.
    ///
    /// A parked claim has no live lease (`park` clears it, matching the
    /// local `needs_input` transition) but its scope is still exclusively
    /// held pending an answer, so it counts as active here exactly like a
    /// live-leased one -- otherwise a park would silently free the same
    /// files for someone else's `delegate` to take mid-question.
    pub async fn list_active(
        &self,
        tenant_id: &str,
        repository_id: &str,
        now: SystemTime,
    ) -> Result<Vec<ActiveClaim>, ClaimStoreError> {
        let rows = self
            .connection()
            .await?
            .query(
                "SELECT task_id, owner_id, branch, lease_expires_at, paths, symbols \
                 FROM delegated_claims \
                 WHERE tenant_id = $1 AND repository_id = $2 \
                   AND (lease_expires_at >= $3 OR parked) \
                 ORDER BY task_id ASC",
                &[&tenant_id, &repository_id, &now],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| ActiveClaim {
                task_id: row.get(0),
                owner_id: row.get(1),
                branch: row.get(2),
                lease_expires_at: row.get(3),
                paths: row.get(4),
                symbols: row.get(5),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// THE BUG THIS PREVENTS. `delegate`'s `SELECT ... FOR UPDATE` row lock
    /// lives on the *connection*, and holds only until that connection's
    /// transaction ends. Before ADR-0143 a process-wide `Mutex<ClaimStore>`
    /// meant two delegates in one process could never reach the database
    /// together at all, so nothing here ever exercised the lock; the store now
    /// takes `&self` and the mutex is gone, which makes this the first time
    /// two claims for one task genuinely race inside a single process.
    ///
    /// If a later change checks a connection out per statement rather than
    /// once per transaction (decision 4), the lock is released between the
    /// SELECT and the UPDATE and BOTH callers are granted the same task -- a
    /// lost update, and precisely the outcome the CAS exists to prevent. A
    /// test that called `delegate` twice in sequence would keep passing
    /// through that regression, because the second call would simply read the
    /// first one's committed row.
    #[tokio::test]
    async fn two_concurrent_delegates_for_one_task_produce_exactly_one_winner() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-race-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let store = Arc::new(ClaimStore::connect(&pool).await.unwrap());

        let first = request(&tenant_id, task_id, "owner-one");
        let second = request(&tenant_id, task_id, "owner-two");
        let one = tokio::spawn({
            let store = Arc::clone(&store);
            async move { store.delegate(&first, now).await }
        });
        let two = tokio::spawn({
            let store = Arc::clone(&store);
            async move { store.delegate(&second, now).await }
        });
        let one = one
            .await
            .unwrap()
            .expect("the first delegate must not error");
        let two = two
            .await
            .unwrap()
            .expect("the second delegate must not error");

        let winners = [&one, &two]
            .into_iter()
            .filter(|result| result.outcome == ClaimLeaseOutcome::Granted)
            .count();
        assert_eq!(winners, 1, "exactly one may win: {one:?} / {two:?}");
        // Both callers must name the same holder. A rejected caller reporting
        // a different owner than the winner has read a row the winner had not
        // committed yet, which is the lock failing without the count changing.
        assert_eq!(
            one.owner_id, two.owner_id,
            "both callers must agree who holds the claim: {one:?} / {two:?}"
        );

        let active = store
            .list_active(&tenant_id, "repository", now)
            .await
            .unwrap();
        assert_eq!(active.len(), 1, "one task, one durable claim row");
        assert_eq!(
            active[0].owner_id, one.owner_id,
            "the durable row must agree with what both callers were told"
        );
    }

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

    fn recover_request(
        tenant_id: &str,
        task_id: &str,
        expected_owner: &str,
        owner_id: &str,
        reason: &str,
    ) -> ClaimRecoverRequest {
        ClaimRecoverRequest {
            tenant_id: tenant_id.to_owned(),
            repository_id: "repository".to_owned(),
            task_id: task_id.to_owned(),
            expected_owner: expected_owner.to_owned(),
            owner_id: owner_id.to_owned(),
            reason: reason.to_owned(),
            branch: format!("branch/{owner_id}"),
            lease: Duration::from_secs(300),
            paths: vec![format!("src/{owner_id}.rs")],
            symbols: vec![format!("symbol:{owner_id}")],
        }
    }

    #[tokio::test]
    async fn authoritative_store_refuses_a_live_competitor_and_allows_expired_reclaim() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-store-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let second = request(&tenant_id, task_id, "owner-two");
        let store = ClaimStore::connect(&pool).await.unwrap();

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
    async fn the_live_owner_can_release_at_expiry_and_a_different_owner_claims_immediately() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-release-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let second = request(&tenant_id, task_id, "owner-two");
        let store = ClaimStore::connect(&pool).await.unwrap();

        store.delegate(&first, now).await.unwrap();

        // Without a release, a different owner is refused while the lease is live.
        let still_live = store
            .delegate(&second, now + Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(still_live.outcome, ClaimLeaseOutcome::Rejected);

        // Lodestar defines `lease_expires_at == now` as still live. Releasing at
        // that inclusive boundary must agree rather than silently treating the
        // claim as already stranded.
        let at_expiry = now + Duration::from_secs(60);
        let released = store
            .release(&tenant_id, "repository", task_id, "owner-one", at_expiry)
            .await
            .unwrap();
        assert!(released, "the live owner must be able to release at expiry");

        // Immediately grantable to a different owner, with no wait for the
        // original 60-second lease to naturally expire.
        let granted_after_release = store
            .delegate(&second, at_expiry + Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(granted_after_release.outcome, ClaimLeaseOutcome::Granted);
        assert_eq!(granted_after_release.owner_id, "owner-two");
    }

    #[tokio::test]
    async fn a_non_owner_release_is_refused_and_does_not_affect_the_live_claim() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-release-refused-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let store = ClaimStore::connect(&pool).await.unwrap();

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
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-release-expired-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let store = ClaimStore::connect(&pool).await.unwrap();

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
    async fn a_parked_claim_blocks_delegate_and_recover_until_its_owner_answers() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-park-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let store = ClaimStore::connect(&pool).await.unwrap();

        store.delegate(&first, now).await.unwrap();

        let parked = store
            .park(
                &tenant_id,
                "repository",
                task_id,
                "owner-one",
                now + Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert!(parked, "the current owner must be able to park its claim");

        // Parking again before an answer is refused: a second park could
        // silently overwrite who is actually waiting on the reply.
        let parked_again = store
            .park(
                &tenant_id,
                "repository",
                task_id,
                "owner-one",
                now + Duration::from_secs(2),
            )
            .await
            .unwrap();
        assert!(!parked_again, "parking an already-parked claim is refused");

        // A different owner cannot delegate over a parked claim, even though
        // the underlying lease was cleared and would otherwise read as
        // expired.
        let blocked = store
            .delegate(
                &request(&tenant_id, task_id, "owner-two"),
                now + Duration::from_secs(3),
            )
            .await
            .unwrap();
        assert_eq!(blocked.outcome, ClaimLeaseOutcome::Rejected);
        assert_eq!(blocked.owner_id, "owner-one");

        // Nor can a rescuer recover it: only `answer` may resolve a park.
        let recover_blocked = store
            .recover(
                &recover_request(&tenant_id, task_id, "owner-one", "owner-two", "rescuing"),
                now + Duration::from_secs(4),
            )
            .await
            .unwrap();
        assert_eq!(recover_blocked.outcome, ClaimLeaseOutcome::Rejected);

        // A different agent's answer is refused: only the parking owner may
        // resume.
        let wrong_answerer = store
            .answer(
                &tenant_id,
                "repository",
                task_id,
                "owner-two",
                Duration::from_secs(60),
                now + Duration::from_secs(5),
            )
            .await
            .unwrap();
        assert_eq!(wrong_answerer.outcome, ClaimLeaseOutcome::Rejected);

        let answered = store
            .answer(
                &tenant_id,
                "repository",
                task_id,
                "owner-one",
                Duration::from_secs(60),
                now + Duration::from_secs(6),
            )
            .await
            .unwrap();
        assert_eq!(answered.outcome, ClaimLeaseOutcome::Granted);
        assert_eq!(answered.owner_id, "owner-one");
        assert_eq!(answered.paths, first.paths, "scope survives the round trip");

        // Un-parked and live again: a different owner is now refused for the
        // ordinary reason (a live lease), not because it is still parked.
        let live_again = store
            .delegate(
                &request(&tenant_id, task_id, "owner-two"),
                now + Duration::from_secs(7),
            )
            .await
            .unwrap();
        assert_eq!(live_again.outcome, ClaimLeaseOutcome::Rejected);
        assert_eq!(live_again.owner_id, "owner-one");
    }

    #[tokio::test]
    async fn answering_a_never_parked_claim_is_refused() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-answer-unparked-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let store = ClaimStore::connect(&pool).await.unwrap();

        store.delegate(&first, now).await.unwrap();

        let rejected = store
            .answer(
                &tenant_id,
                "repository",
                task_id,
                "owner-one",
                Duration::from_secs(60),
                now + Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert_eq!(
            rejected.outcome,
            ClaimLeaseOutcome::Rejected,
            "a live, never-parked claim has nothing for answer to resolve"
        );
    }

    #[tokio::test]
    async fn the_live_owner_can_renew_and_the_started_at_and_scope_are_untouched() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-renew-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let store = ClaimStore::connect(&pool).await.unwrap();

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
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-renew-refused-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let store = ClaimStore::connect(&pool).await.unwrap();

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
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-renew-expired-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let store = ClaimStore::connect(&pool).await.unwrap();

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

    #[tokio::test]
    async fn an_expired_claim_can_be_recovered_by_a_new_owner_with_a_reason() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-recover-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let store = ClaimStore::connect(&pool).await.unwrap();

        store.delegate(&first, now).await.unwrap();

        // Past the 60-second lease: genuinely stranded.
        let recovered = store
            .recover(
                &recover_request(
                    &tenant_id,
                    task_id,
                    "owner-one",
                    "owner-two",
                    "owner-one went silent",
                ),
                now + Duration::from_secs(61),
            )
            .await
            .unwrap();

        assert_eq!(recovered.outcome, ClaimLeaseOutcome::Granted);
        assert_eq!(recovered.owner_id, "owner-two");
        assert_eq!(recovered.branch, "branch/owner-two");
        assert_eq!(recovered.claim_lapses, 1, "a lapse is recorded, not hidden");
        assert_eq!(recovered.paths, vec!["src/owner-two.rs".to_owned()]);
    }

    #[tokio::test]
    async fn recovering_a_still_live_claim_is_refused() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-recover-live-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let store = ClaimStore::connect(&pool).await.unwrap();

        let granted = store.delegate(&first, now).await.unwrap();

        let rejected = store
            .recover(
                &recover_request(
                    &tenant_id,
                    task_id,
                    "owner-one",
                    "owner-two",
                    "trying to take over early",
                ),
                now + Duration::from_secs(1),
            )
            .await
            .unwrap();

        assert_eq!(
            rejected.outcome,
            ClaimLeaseOutcome::Rejected,
            "a live lease is never recoverable out from under its holder"
        );
        assert_eq!(rejected.owner_id, "owner-one");
        assert_eq!(rejected.lease_expires_at, granted.lease_expires_at);
    }

    #[tokio::test]
    async fn the_same_owner_can_recover_their_own_expired_claim_and_started_at_is_preserved() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-recover-self-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let store = ClaimStore::connect(&pool).await.unwrap();

        let granted = store.delegate(&first, now).await.unwrap();

        let recovered = store
            .recover(
                &recover_request(
                    &tenant_id,
                    task_id,
                    "owner-one",
                    "owner-one",
                    "reconnecting after a network partition",
                ),
                now + Duration::from_secs(61),
            )
            .await
            .unwrap();

        assert_eq!(recovered.outcome, ClaimLeaseOutcome::Granted);
        assert_eq!(
            recovered.claim_started_at, granted.claim_started_at,
            "a same-owner recovery must preserve claim_started_at"
        );
        assert_eq!(
            recovered.branch, granted.branch,
            "a same-owner recovery must preserve the declared branch"
        );
    }

    #[tokio::test]
    async fn a_mismatched_expected_owner_is_refused_even_though_the_lease_expired() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-recover-mismatch-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let store = ClaimStore::connect(&pool).await.unwrap();

        store.delegate(&first, now).await.unwrap();

        let rejected = store
            .recover(
                &recover_request(
                    &tenant_id,
                    task_id,
                    "owner-wrong-guess",
                    "owner-three",
                    "taking over what I believe is stranded",
                ),
                now + Duration::from_secs(61),
            )
            .await
            .unwrap();

        assert_eq!(
            rejected.outcome,
            ClaimLeaseOutcome::Rejected,
            "the owner changed concurrently from what the caller expected"
        );
        assert_eq!(rejected.owner_id, "owner-one");
    }

    #[tokio::test]
    async fn recovery_without_a_reason_is_refused() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!(
            "claim-recover-no-reason-{}",
            crate::test_support::uuid_ish()
        );
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let first = request(&tenant_id, task_id, "owner-one");
        let store = ClaimStore::connect(&pool).await.unwrap();

        store.delegate(&first, now).await.unwrap();

        let error = store
            .recover(
                &recover_request(&tenant_id, task_id, "owner-one", "owner-two", "   "),
                now + Duration::from_secs(61),
            )
            .await
            .unwrap_err();

        assert!(matches!(error, ClaimStoreError::MissingReason));
    }

    #[tokio::test]
    async fn list_active_reports_every_unexpired_claim_scoped_to_its_tenant_and_repository() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-list-active-{}", crate::test_support::uuid_ish());
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let store = ClaimStore::connect(&pool).await.unwrap();

        store
            .delegate(&request(&tenant_id, "task-one", "owner-one"), now)
            .await
            .unwrap();
        store
            .delegate(&request(&tenant_id, "task-two", "owner-two"), now)
            .await
            .unwrap();
        // A different tenant's claim must never appear in this tenant's list.
        store
            .delegate(
                &request(&format!("{tenant_id}-other"), "task-one", "owner-three"),
                now,
            )
            .await
            .unwrap();

        let active = store
            .list_active(&tenant_id, "repository", now)
            .await
            .unwrap();

        assert_eq!(active.len(), 2);
        assert_eq!(active[0].task_id, "task-one");
        assert_eq!(active[0].owner_id, "owner-one");
        assert_eq!(active[0].branch, "branch/owner-one");
        assert_eq!(active[0].paths, vec!["src/owner-one.rs".to_string()]);
        assert_eq!(active[0].lease_expires_at, now + Duration::from_secs(60));
        assert_eq!(active[1].task_id, "task-two");
        assert_eq!(active[1].owner_id, "owner-two");
    }

    #[tokio::test]
    async fn list_active_excludes_a_claim_whose_lease_has_expired() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = format!("claim-list-expired-{}", crate::test_support::uuid_ish());
        let task_id = "task";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let store = ClaimStore::connect(&pool).await.unwrap();

        store
            .delegate(&request(&tenant_id, task_id, "owner-one"), now)
            .await
            .unwrap();

        let still_active = store
            .list_active(&tenant_id, "repository", now + Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(still_active.len(), 1);

        let at_expiry = store
            .list_active(&tenant_id, "repository", now + Duration::from_secs(60))
            .await
            .unwrap();
        assert_eq!(
            at_expiry.len(),
            1,
            "the inclusive expiry boundary is still a live claim"
        );

        // Past the 60-second lease: nothing live remains to report.
        let after_expiry = store
            .list_active(&tenant_id, "repository", now + Duration::from_secs(61))
            .await
            .unwrap();
        assert!(
            after_expiry.is_empty(),
            "an expired claim must not appear in the active list"
        );
    }

    #[tokio::test]
    async fn a_nonce_is_consumed_exactly_once_per_signing_key() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let signing_key_id = format!("claim-nonce-{}", crate::test_support::uuid_ish());
        let nonce = b"a fixed nonce value".to_vec();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let store = ClaimStore::connect(&pool).await.unwrap();

        let first = store
            .consume_claim_nonce(&signing_key_id, &nonce, now)
            .await
            .unwrap();
        assert!(
            first,
            "a fresh (signing_key_id, nonce) pair must be consumable"
        );

        let second = store
            .consume_claim_nonce(&signing_key_id, &nonce, now + Duration::from_secs(1))
            .await
            .unwrap();
        assert!(
            !second,
            "the identical (signing_key_id, nonce) pair must never be consumable twice"
        );

        // A different nonce under the same key is an unrelated pair and is
        // still fresh -- the primary key is the *pair*, not the key alone.
        let different_nonce = store
            .consume_claim_nonce(&signing_key_id, b"a different nonce value", now)
            .await
            .unwrap();
        assert!(
            different_nonce,
            "a different nonce under the same signing key is its own, unconsumed pair"
        );
    }
}
