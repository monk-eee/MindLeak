//! Process-wide `tracing` subscriber setup.

/// Install the process-wide `tracing` subscriber: **stderr only**, env-gated.
///
/// Safe to call once from a binary's `main`; a second call is a no-op. Reads
/// `MINDLEAK_LOG` (filter, default `info`) and `MINDLEAK_LOG_FORMAT`
/// (`pretty` | `json`, default `pretty`). Never writes to stdout.
pub fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_env("MINDLEAK_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let json = std::env::var("MINDLEAK_LOG_FORMAT")
        .map(|v| v.eq_ignore_ascii_case("json"))
        .unwrap_or(false);

    let builder = fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(true);

    // try_init is a no-op (Err) if a global subscriber is already set.
    let _ = if json {
        builder.json().try_init()
    } else {
        builder.try_init()
    };
}
