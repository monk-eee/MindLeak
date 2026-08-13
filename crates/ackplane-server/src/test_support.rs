//! Shared test-only helpers for this crate's database-gated tests.

/// A cheap, dependency-free way to keep each test run's tenant id unique
/// without adding a `uuid` crate for two words of randomness.
pub(crate) fn uuid_ish() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}
