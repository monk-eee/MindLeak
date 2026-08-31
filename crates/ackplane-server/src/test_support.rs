//! Shared test-only helpers for this crate's database-gated tests.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::SystemTime;

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

use crate::db_pool::{build_pool, PgPool, TEST_POOL_MAX_SIZE};
use crate::enrollment::{activation_challenge_bytes, public_key_fingerprint};
use crate::enrollment_store::{
    ActivationChallengeRequest, EnrollmentActivation, EnrollmentApproval, EnrollmentStore,
    EnrollmentSubmission,
};

/// A bounded pool for one database-gated test, or `None` when the suite is not
/// gated on.
///
/// Built per test rather than once per binary, which is a deliberate departure
/// from ADR-0143 decision 7's "one pool per test binary". `#[tokio::test]`
/// gives every test its own runtime, and a `tokio_postgres` connection is
/// driven by a task spawned on the runtime that opened it. A pool shared across
/// runtimes therefore hands the second test a connection whose driver died with
/// the first test's runtime -- measured here as
/// `Database(Error { kind: Closed })` raised from whatever query happened to
/// run next, naming nothing to do with pooling. Decision 7's intent still
/// holds: demand is bounded per test and connections go back to the pool
/// between calls instead of one being held for the whole test.
pub(crate) fn test_pool() -> Option<PgPool> {
    let database_url = std::env::var("ACKPLANE_TEST_DATABASE_URL").ok()?;
    Some(
        build_pool(&database_url, TEST_POOL_MAX_SIZE)
            .expect("the test pool builds from a valid database url"),
    )
}

/// The same pool for a test that has already checked the gate itself, and so
/// still needs the url for a store that has not moved to the pool yet.
pub(crate) fn gated_test_pool() -> PgPool {
    test_pool().expect("ACKPLANE_TEST_DATABASE_URL was checked by the caller")
}

/// A cheap, dependency-free way to keep each test run's tenant id unique
/// without adding a `uuid` crate for two words of randomness. Packs a
/// monotonic per-process counter into the low 32 bits alongside wall-clock
/// nanoseconds, so two calls from concurrently running tests in the same
/// test binary can never return the same value even when the system clock's
/// effective resolution is coarser than the gap between them -- measured:
/// `fleet.rs` tests running in parallel threads, each calling this once for
/// their own tenant/repository/request ids, collided on a bare timestamp
/// roughly 1 run in 4 (`enrollment_requests_pkey` duplicate key).
pub(crate) fn uuid_ish() -> u128 {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    (nanos << 32) | u128::from(counter)
}

/// A 32-byte activation-challenge nonce that is unique both within a single
/// test binary invocation (an atomic counter guards two calls made in the
/// same nanosecond) and across repeated runs against the same persistent
/// `ACKPLANE_TEST_DATABASE_URL` container (seeded from wall-clock
/// nanoseconds). `activation_challenges.nonce` carries a bare unique
/// constraint, so a fixed literal nonce collides on the container's second
/// run rather than the first.
/// Real callers generate this the same way, via `getrandom`, in
/// `enrollment_service.rs`; tests use a counter instead so the fixture stays
/// dependency-free and deterministic to read in a failure message.
pub(crate) fn unique_nonce() -> [u8; 32] {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = uuid_ish();
    let mut nonce = [0_u8; 32];
    nonce[..16].copy_from_slice(&timestamp.to_be_bytes());
    nonce[16..24].copy_from_slice(&counter.to_be_bytes());
    nonce
}

/// A `prefix` suffixed with a wall-clock-seeded, atomic-counter-guarded
/// identifier -- unique within one test binary invocation and across repeated
/// runs against the same persistent `ACKPLANE_TEST_DATABASE_URL` container,
/// same reasoning as [`unique_nonce`]. For primary-key-like string columns
/// (e.g. `enrollment_receipts.enrollment_receipt_id`) where a fixed literal
/// collides on the container's second run.
pub(crate) fn unique_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{counter}", uuid_ish())
}

/// The maintenance connection string for provisioning ephemeral scratch
/// databases in rehearsal/recovery-execution tests, or `None` when the suite
/// is not gated on it.
pub(crate) fn rehearsal_test_url() -> Option<String> {
    std::env::var("ACKPLANE_TEST_REHEARSAL_DATABASE_URL").ok()
}

/// Creates and drops an ephemeral database against `maintenance_url`, and
/// hands `body` its own connection url -- used so rehearsal and recovery-
/// execution integration tests never touch `ACKPLANE_TEST_DATABASE_URL`'s
/// shared migration state: tampering a migration digest there could break
/// every other test or fleet agent sharing that database. Shared by
/// `snapshot_provider`'s rehearsal tests and `recovery_execution_receipt_write`'s
/// full-orchestration restore test rather than duplicated in each.
pub(crate) async fn with_ephemeral_database<Fut>(
    maintenance_url: &str,
    name_prefix: &str,
    body: impl FnOnce(String) -> Fut,
) where
    Fut: std::future::Future<Output = ()>,
{
    use crate::snapshot_provider::{unique_suffix, with_dbname};

    let name = format!("{}_{}", name_prefix, unique_suffix());
    let (client, connection) = tokio_postgres::connect(maintenance_url, tokio_postgres::NoTls)
        .await
        .expect("a direct maintenance connection should succeed");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
        .batch_execute(&format!("CREATE DATABASE \"{name}\""))
        .await
        .expect("creating the ephemeral fixture database should succeed");

    let url = with_dbname(maintenance_url, &name)
        .expect("the rehearsal test url should be a postgresql:// uri");
    body(url).await;

    let _ = client
        .batch_execute(&format!("DROP DATABASE IF EXISTS \"{name}\""))
        .await;
}

/// Enrolls and activates one node for `tenant_id`/`repository_id`, shared by
/// every store's tests needing an active Fleet entry (originally
/// `fleet.rs`'s own private helper; extracted here once `readiness.rs`
/// needed the identical ceremony rather than a second copy). `nonce_seed`
/// must be unique per call: the activation-challenge nonce carries a GLOBAL
/// uniqueness constraint, not one scoped per tenant.
pub(crate) async fn enroll_and_activate_in(
    database_url: &str,
    tenant_id: &str,
    repository_id: &str,
    nonce_seed: &str,
) {
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let public_key = signing_key.verifying_key().to_bytes();
    let submission = EnrollmentSubmission {
        request_id: format!("fleet-request-{nonce_seed}"),
        tenant_id: tenant_id.to_string(),
        repository_id: repository_id.to_string(),
        proposed_node_id: format!("fleet-node-{nonce_seed}"),
        display_name: "Fleet test node".to_string(),
        public_key: public_key.to_vec(),
        public_key_fingerprint: public_key_fingerprint(&public_key),
        requested_capabilities: vec!["synchronize".to_string()],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        expires_at: "2030-01-01T00:00:00Z".to_string(),
    };
    let request = ActivationChallengeRequest {
        request_id: submission.request_id.clone(),
        tenant_id: tenant_id.to_string(),
        repository_id: repository_id.to_string(),
        proposed_node_id: submission.proposed_node_id.clone(),
        public_key_fingerprint: submission.public_key_fingerprint.clone(),
    };
    let now = SystemTime::now();
    let pool = build_pool(database_url, TEST_POOL_MAX_SIZE)
        .expect("the test pool builds from a valid database url");
    let enrollment = EnrollmentStore::connect(&pool)
        .await
        .expect("connect enrollment store");
    enrollment
        .submit(&submission)
        .await
        .expect("submit enrollment");
    enrollment
        .approve(&EnrollmentApproval {
            request_id: submission.request_id.clone(),
            tenant_id: tenant_id.to_string(),
            repository_id: repository_id.to_string(),
            public_key_fingerprint: submission.public_key_fingerprint.clone(),
            approved_capabilities: submission.requested_capabilities.clone(),
            approved_by: "fleet-test-administrator".to_string(),
        })
        .await
        .expect("approve enrollment");
    // `activation_challenges.nonce` carries a GLOBAL unique constraint (not
    // scoped per tenant/repo), so a hardcoded literal collides the moment
    // this helper is called from more than one test in the same process.
    // Derive it from `nonce_seed` instead.
    let nonce: [u8; 32] = Sha256::digest(nonce_seed.as_bytes()).into();
    let challenge = enrollment
        .issue_challenge(&request, &nonce, now)
        .await
        .expect("issue activation challenge");
    let signature = signing_key.sign(&activation_challenge_bytes(
        &challenge.nonce,
        &request.request_id,
        &request.tenant_id,
        &request.repository_id,
        &request.proposed_node_id,
        &request.public_key_fingerprint,
    ));
    enrollment
        .activate(
            &EnrollmentActivation {
                request,
                nonce: challenge.nonce,
                signature: signature.to_bytes().to_vec(),
            },
            &format!("fleet-receipt-{nonce_seed}"),
            &format!("fleet-signing-key-{nonce_seed}"),
            now,
        )
        .await
        .expect("activate enrollment");
}

/// Enrolls and activates one node for a fresh tenant/repository pair, so
/// each test starts from an active Fleet entry without repeating the
/// enrolment ceremony inline.
pub(crate) async fn enroll_and_activate(database_url: &str, unique_id: &str) -> (String, String) {
    let tenant_id = format!("fleet-tenant-{unique_id}");
    let repository_id = format!("fleet-repository-{unique_id}");
    enroll_and_activate_in(database_url, &tenant_id, &repository_id, unique_id).await;
    (tenant_id, repository_id)
}
