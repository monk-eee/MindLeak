//! Network resilience for optional outbound HTTP (ADR-0010).
//!
//! Only *optional* endpoints route through here — LLM consolidation and the
//! embedding index. The deterministic ingest/query path never touches the
//! network and therefore never depends on any of this. Every call gets an
//! explicit timeout, bounded retry with exponential backoff on transient
//! failures, and a per-endpoint circuit breaker so a degraded server fast-fails
//! instead of stalling the agent. State transitions are traced, never printed to
//! stdout (which carries the JSON-RPC protocol).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{MindLeakError, Result};

/// Tunable network policy, read once from the environment (ADR-0010 defaults).
#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub timeout: Duration,
    pub retries: u32,
    pub breaker_threshold: u32,
    pub breaker_cooldown: Duration,
}

impl Default for HttpConfig {
    fn default() -> Self {
        HttpConfig {
            timeout: Duration::from_millis(env_u64("MINDLEAK_HTTP_TIMEOUT_MS", 30_000)),
            retries: env_u64("MINDLEAK_HTTP_RETRIES", 2) as u32,
            breaker_threshold: env_u64("MINDLEAK_BREAKER_THRESHOLD", 5) as u32,
            breaker_cooldown: Duration::from_millis(env_u64(
                "MINDLEAK_BREAKER_COOLDOWN_MS",
                30_000,
            )),
        }
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Circuit-breaker state for one endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl CircuitState {
    pub fn as_str(&self) -> &'static str {
        match self {
            CircuitState::Closed => "closed",
            CircuitState::Open => "open",
            CircuitState::HalfOpen => "half_open",
        }
    }
}

/// A single endpoint's breaker. Pure and clock-injectable so it unit-tests
/// without sleeping or hitting the network.
#[derive(Debug)]
pub struct CircuitBreaker {
    threshold: u32,
    cooldown: Duration,
    failures: u32,
    state: CircuitState,
    opened_at: Option<Instant>,
}

impl CircuitBreaker {
    pub fn new(threshold: u32, cooldown: Duration) -> Self {
        CircuitBreaker {
            threshold: threshold.max(1),
            cooldown,
            failures: 0,
            state: CircuitState::Closed,
            opened_at: None,
        }
    }

    /// Whether a call may proceed at `now`. An open circuit transitions to
    /// half-open (allowing a single probe) once the cooldown has elapsed.
    pub fn allow(&mut self, now: Instant) -> bool {
        match self.state {
            CircuitState::Closed | CircuitState::HalfOpen => true,
            CircuitState::Open => {
                let elapsed = self
                    .opened_at
                    .map(|t| now.duration_since(t))
                    .unwrap_or(self.cooldown);
                if elapsed >= self.cooldown {
                    self.state = CircuitState::HalfOpen;
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Record a success: the endpoint is healthy, close the circuit.
    pub fn on_success(&mut self) {
        self.failures = 0;
        self.state = CircuitState::Closed;
        self.opened_at = None;
    }

    /// Record a failure. A failed half-open probe reopens immediately; otherwise
    /// the circuit opens once consecutive failures reach the threshold.
    pub fn on_failure(&mut self, now: Instant) {
        self.failures = self.failures.saturating_add(1);
        if self.state == CircuitState::HalfOpen || self.failures >= self.threshold {
            self.state = CircuitState::Open;
            self.opened_at = Some(now);
        }
    }

    pub fn state(&self) -> CircuitState {
        self.state
    }
}

/// Process-wide per-endpoint breaker registry. The MCP server is one long-lived
/// process handling many calls, so breaker state must outlive individual calls.
fn breakers() -> &'static Mutex<HashMap<String, CircuitBreaker>> {
    static BREAKERS: OnceLock<Mutex<HashMap<String, CircuitBreaker>>> = OnceLock::new();
    BREAKERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// POST `body` as JSON to `url`, returning the parsed JSON response.
///
/// Applies the timeout, bounded retry with backoff, and the per-endpoint circuit
/// breaker. Returns a typed `Http` error (never a panic, never a hang) on
/// failure or when the circuit is open.
pub fn post_json(cfg: &HttpConfig, url: &str, api_key: &str, body: &Value) -> Result<Value> {
    if !breaker_allows(cfg, url) {
        tracing::warn!(target: "mindleak::net", %url, "circuit open; fast-failing without a request");
        return Err(MindLeakError::Http(format!("circuit open for {url}")));
    }

    let agent = ureq::builder()
        .timeout_connect(cfg.timeout)
        .timeout_read(cfg.timeout)
        .build();

    let mut attempt = 0u32;
    loop {
        attempt += 1;
        let mut req = agent.post(url);
        if !api_key.is_empty() {
            req = req.set("Authorization", &format!("Bearer {api_key}"));
        }
        match req.send_json(body) {
            Ok(resp) => {
                let value = resp
                    .into_json::<Value>()
                    .map_err(|e| MindLeakError::Http(e.to_string()))?;
                record_success(url);
                return Ok(value);
            }
            Err(err) => {
                let transient = is_transient(&err);
                tracing::warn!(target: "mindleak::net", %url, attempt, transient, error = %err, "http attempt failed");
                if transient && attempt <= cfg.retries {
                    std::thread::sleep(backoff(attempt));
                    continue;
                }
                record_failure(url, cfg);
                return Err(MindLeakError::Http(err.to_string()));
            }
        }
    }
}

fn breaker_allows(cfg: &HttpConfig, url: &str) -> bool {
    let mut map = breakers().lock().unwrap();
    let cb = map
        .entry(url.to_string())
        .or_insert_with(|| CircuitBreaker::new(cfg.breaker_threshold, cfg.breaker_cooldown));
    cb.allow(Instant::now())
}

fn record_success(url: &str) {
    if let Ok(mut map) = breakers().lock() {
        if let Some(cb) = map.get_mut(url) {
            cb.on_success();
        }
    }
}

fn record_failure(url: &str, cfg: &HttpConfig) {
    if let Ok(mut map) = breakers().lock() {
        let cb = map
            .entry(url.to_string())
            .or_insert_with(|| CircuitBreaker::new(cfg.breaker_threshold, cfg.breaker_cooldown));
        let before = cb.state();
        cb.on_failure(Instant::now());
        if before != CircuitState::Open && cb.state() == CircuitState::Open {
            tracing::warn!(target: "mindleak::net", %url, "circuit opened after repeated failures");
        }
    }
}

fn is_transient(err: &ureq::Error) -> bool {
    match err {
        // 5xx is worth retrying; 4xx is a client error and is not.
        ureq::Error::Status(code, _) => *code >= 500,
        // connect/read timeout, connection refused, DNS, TLS, etc.
        ureq::Error::Transport(_) => true,
    }
}

/// Exponential backoff: 100ms, 200ms, 400ms, ... capped at 2s.
fn backoff(attempt: u32) -> Duration {
    let shift = attempt.clamp(1, 5) - 1;
    let ms = 100u64.saturating_mul(1u64 << shift);
    Duration::from_millis(ms.min(2000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breaker_opens_after_threshold_then_recovers_on_probe() {
        let cooldown = Duration::from_secs(30);
        let mut cb = CircuitBreaker::new(3, cooldown);
        let t0 = Instant::now();

        // Below threshold the circuit stays closed and keeps allowing calls.
        assert!(cb.allow(t0));
        cb.on_failure(t0);
        cb.on_failure(t0);
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow(t0));

        // The third consecutive failure trips it open; calls fast-fail.
        cb.on_failure(t0);
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow(t0));
        assert!(!cb.allow(t0 + Duration::from_secs(29)));

        // After the cooldown a single half-open probe is allowed; success closes.
        assert!(cb.allow(t0 + Duration::from_secs(31)));
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        cb.on_success();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow(t0 + Duration::from_secs(31)));
    }

    #[test]
    fn failed_half_open_probe_reopens_immediately() {
        let cooldown = Duration::from_secs(10);
        let mut cb = CircuitBreaker::new(2, cooldown);
        let t0 = Instant::now();

        cb.on_failure(t0);
        cb.on_failure(t0);
        assert_eq!(cb.state(), CircuitState::Open);

        // Cooldown elapses -> half-open probe allowed.
        assert!(cb.allow(t0 + Duration::from_secs(11)));
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        // Probe fails -> straight back to open, without waiting for the threshold.
        cb.on_failure(t0 + Duration::from_secs(11));
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow(t0 + Duration::from_secs(12)));
    }

    #[test]
    fn backoff_is_bounded_and_monotonic() {
        assert_eq!(backoff(1), Duration::from_millis(100));
        assert_eq!(backoff(2), Duration::from_millis(200));
        assert_eq!(backoff(3), Duration::from_millis(400));
        assert!(backoff(10) <= Duration::from_millis(2000));
    }
}
