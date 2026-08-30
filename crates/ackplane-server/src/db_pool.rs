//! One bounded PostgreSQL connection pool per process (ADR-0143).
//!
//! Every store used to open and keep its own dedicated `tokio_postgres::Client`
//! for its whole lifetime. That is fine per running service — the count is
//! small and deterministic — but it makes demand unbounded under test, where
//! each DB-gated test constructs its own stores and each store opened another
//! raw connection. `cargo test --all` exhausted Postgres's connection ceiling
//! that way, and the failure was actively misleading: the panicking test names
//! never mention connections, only the panic body does, so it read as several
//! unrelated subsystems breaking at once.
//!
//! The pool bounds demand per *process* rather than per *store*, so no number
//! of stores or tasks a single process starts can exhaust a shared instance.

use std::time::Duration;

use deadpool_postgres::{Config, ManagerConfig, PoolConfig, RecyclingMethod, Runtime};
use tokio_postgres::NoTls;

/// A checked-out connection. Derefs to `tokio_postgres::Client`, so store code
/// keeps its existing query signatures unchanged.
pub type PgConnection = deadpool_postgres::Object;

pub type PgPool = deadpool_postgres::Pool;

/// Default cap for a long-running service binary.
pub const SERVICE_POOL_MAX_SIZE: usize = 16;

/// Default cap for a test-fixture process. Lower than a service's because many
/// test binaries run concurrently, and it is their *sum* that has to stay under
/// the development ceiling.
pub const TEST_POOL_MAX_SIZE: usize = 8;

const DEFAULT_TIMEOUT_MS: u64 = 5_000;

const MAX_SIZE_VAR: &str = "ACKPLANE_DB_POOL_MAX_SIZE";
const TIMEOUT_VAR: &str = "ACKPLANE_DB_POOL_TIMEOUT_MS";

#[derive(Debug, thiserror::Error)]
pub enum PoolBuildError {
    #[error("{variable} must be a positive integer, not {value:?}")]
    InvalidSetting {
        variable: &'static str,
        value: String,
    },
    #[error("building the PostgreSQL pool failed: {0}")]
    Create(#[from] deadpool_postgres::CreatePoolError),
}

/// Build this process's single pool. Hand every store a clone of the returned
/// handle rather than calling this again — it is an `Arc` internally, and a
/// second pool would reintroduce exactly the unbounded demand this bounds.
pub fn build_pool(database_url: &str, default_max_size: usize) -> Result<PgPool, PoolBuildError> {
    let max_size = resolve_positive(
        std::env::var(MAX_SIZE_VAR).ok().as_deref(),
        default_max_size,
        MAX_SIZE_VAR,
    )?;
    let timeout_ms = resolve_positive(
        std::env::var(TIMEOUT_VAR).ok().as_deref(),
        DEFAULT_TIMEOUT_MS as usize,
        TIMEOUT_VAR,
    )?;

    let mut config = Config::new();
    config.url = Some(database_url.to_string());
    config.manager = Some(ManagerConfig {
        // Verified on checkout rather than trusted: a connection the server
        // closed while it sat idle in the pool is otherwise handed to a caller
        // that then fails for a reason having nothing to do with its query.
        recycling_method: RecyclingMethod::Fast,
    });

    let mut pool_config = PoolConfig::new(max_size);
    // Bounded, so exhaustion is a typed refusal rather than a request that
    // hangs forever (ADR-0143 decision 5).
    pool_config.timeouts.wait = Some(Duration::from_millis(timeout_ms as u64));
    config.pool = Some(pool_config);

    Ok(config.create_pool(Some(Runtime::Tokio1), NoTls)?)
}

/// Parse an optional override, refusing anything that is not a positive
/// integer rather than silently falling back to the default.
///
/// Silently defaulting would be worse than the misconfiguration: an operator
/// who set a cap and got the built-in one instead has no way to tell, and the
/// setting exists precisely for the case where the default is wrong.
fn resolve_positive(
    raw: Option<&str>,
    default: usize,
    variable: &'static str,
) -> Result<usize, PoolBuildError> {
    let Some(raw) = raw else {
        return Ok(default);
    };
    let trimmed = raw.trim();
    match trimmed.parse::<usize>() {
        Ok(value) if value > 0 => Ok(value),
        _ => Err(PoolBuildError::InvalidSetting {
            variable,
            value: raw.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_setting_uses_the_default() {
        assert_eq!(resolve_positive(None, 16, MAX_SIZE_VAR).unwrap(), 16);
    }

    #[test]
    fn a_valid_setting_overrides_the_default() {
        assert_eq!(resolve_positive(Some("32"), 16, MAX_SIZE_VAR).unwrap(), 32);
        assert_eq!(resolve_positive(Some("  4 "), 16, MAX_SIZE_VAR).unwrap(), 4);
    }

    /// Regression: a misconfigured cap must refuse, never quietly become the
    /// default.
    ///
    /// THE BUG THIS PREVENTS. The obvious implementation is
    /// `raw.and_then(|v| v.parse().ok()).unwrap_or(default)`, which turns
    /// `ACKPLANE_DB_POOL_MAX_SIZE=32ted` — or the `0` that means "no
    /// connections at all" — into the built-in default with no signal. The
    /// operator who set it sees a pool that ignores them, and the setting only
    /// exists for the case where the default is already wrong for their
    /// deployment, so the one situation it is needed in is the one where it
    /// silently does nothing.
    #[test]
    fn a_malformed_or_zero_setting_is_refused_rather_than_defaulted() {
        for bad in ["", "abc", "32ted", "0", "-1", "1.5"] {
            let error = resolve_positive(Some(bad), 16, MAX_SIZE_VAR)
                .expect_err("a non-positive-integer setting must be refused");
            assert!(
                matches!(
                    &error,
                    PoolBuildError::InvalidSetting { variable, value }
                        if *variable == MAX_SIZE_VAR && value == bad
                ),
                "expected the refusal to name the variable and the offending value, got {error}"
            );
        }
    }

    /// The refusal has to name which setting is wrong; a process reading two
    /// integer settings cannot be debugged from "must be a positive integer".
    #[test]
    fn the_refusal_names_the_offending_variable() {
        let error = resolve_positive(Some("nope"), 5_000, TIMEOUT_VAR).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains(TIMEOUT_VAR) && message.contains("nope"),
            "expected the message to name {TIMEOUT_VAR} and the bad value, got {message}"
        );
    }

    /// Regression: the cap must reach the pool, not merely be parsed.
    ///
    /// THE BUG THIS PREVENTS. Every other test here exercises `resolve_positive`
    /// in isolation, so all of them keep passing if `build_pool` computes
    /// `max_size` correctly and then never puts it in the `PoolConfig` -- the
    /// pool silently takes deadpool's own default instead. That is invisible
    /// in normal use (a pool with a larger cap works fine) and only shows up
    /// as the connection exhaustion this whole ADR exists to stop. Reading the
    /// built pool's own reported `max_size` is what makes the wiring itself
    /// the thing under test.
    ///
    /// No database is contacted: deadpool creates connections lazily, so the
    /// url only has to parse.
    #[test]
    fn the_configured_cap_reaches_the_built_pool() {
        let pool = build_pool(
            "postgres://user:pw@127.0.0.1:5432/does-not-need-to-exist",
            3,
        )
        .expect("a syntactically valid url builds a pool without contacting the server");

        assert_eq!(pool.status().max_size, 3);
    }

    /// A url the driver cannot parse must fail loudly at build time rather
    /// than deferring to the first query, where it would surface as an
    /// unrelated-looking store error long after the misconfiguration.
    #[test]
    fn an_unparseable_database_url_is_refused_when_the_pool_is_built() {
        let error = build_pool("not-a-postgres-url", SERVICE_POOL_MAX_SIZE)
            .expect_err("an unparseable url must not produce a pool");

        assert!(matches!(error, PoolBuildError::Create(_)), "got {error}");
    }
}
