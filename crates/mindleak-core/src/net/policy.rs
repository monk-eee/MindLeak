use std::time::Duration;

const MIN_TIMEOUT_MS: u64 = 100;
const MAX_TIMEOUT_MS: u64 = 300_000;
const MAX_RETRIES: u64 = 5;

#[derive(Debug, Clone, Copy)]
pub(super) enum RetryPolicy {
    Transient,
    ReadTimeout,
}

/// Tunable network policy, read once from the environment (ADR-0010 defaults).
#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub retries: u32,
    pub breaker_threshold: u32,
    pub breaker_cooldown: Duration,
    pub(super) retry_policy: RetryPolicy,
}

impl Default for HttpConfig {
    fn default() -> Self {
        HttpConfig {
            connect_timeout: Duration::from_millis(env_bounded_u64(
                "MINDLEAK_HTTP_CONNECT_TIMEOUT_MS",
                1_000,
                MIN_TIMEOUT_MS,
                MAX_TIMEOUT_MS,
            )),
            read_timeout: Duration::from_millis(env_bounded_u64(
                "MINDLEAK_HTTP_TIMEOUT_MS",
                30_000,
                MIN_TIMEOUT_MS,
                MAX_TIMEOUT_MS,
            )),
            retries: env_bounded_u64("MINDLEAK_HTTP_RETRIES", 2, 0, MAX_RETRIES) as u32,
            breaker_threshold: env_u64("MINDLEAK_BREAKER_THRESHOLD", 5) as u32,
            breaker_cooldown: Duration::from_millis(env_u64(
                "MINDLEAK_BREAKER_COOLDOWN_MS",
                30_000,
            )),
            retry_policy: RetryPolicy::Transient,
        }
    }
}

impl HttpConfig {
    /// Network policy for the optional local model calls.
    pub fn for_model() -> Self {
        HttpConfig {
            read_timeout: Duration::from_millis(env_bounded_u64(
                "MINDLEAK_MODEL_TIMEOUT_MS",
                120_000,
                MIN_TIMEOUT_MS,
                MAX_TIMEOUT_MS,
            )),
            retries: 1,
            retry_policy: RetryPolicy::ReadTimeout,
            ..HttpConfig::default()
        }
    }

    /// Upper bound for all interruptible work in one retry sequence.
    pub fn maximum_elapsed(&self) -> Duration {
        let attempts = u32::saturating_add(self.retries, 1);
        let dns_budget = self.connect_timeout;
        let attempt_budget = self.connect_timeout.saturating_add(self.read_timeout);
        let request_budget = attempt_budget.saturating_mul(attempts);
        let retry_budget = (1..=self.retries)
            .map(backoff)
            .fold(Duration::ZERO, Duration::saturating_add);
        dns_budget
            .saturating_add(request_budget)
            .saturating_add(retry_budget)
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_bounded_u64(key: &str, default: u64, minimum: u64, maximum: u64) -> u64 {
    env_u64(key, default).clamp(minimum, maximum)
}

/// Exponential backoff: 100ms, 200ms, 400ms, ... capped at 2s.
pub(super) fn backoff(attempt: u32) -> Duration {
    let shift = attempt.clamp(1, 5) - 1;
    let milliseconds = 100u64.saturating_mul(1u64 << shift);
    Duration::from_millis(milliseconds.min(2_000))
}
