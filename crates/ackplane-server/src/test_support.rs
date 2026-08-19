//! Shared test-only helpers for this crate's database-gated tests.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

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
