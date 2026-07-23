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
use std::io::Read;
use std::io::{self};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{MindLeakError, Result};

const MIN_TIMEOUT_MS: u64 = 100;
const MAX_TIMEOUT_MS: u64 = 300_000;
const MAX_RETRIES: u64 = 5;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

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
            timeout: Duration::from_millis(env_bounded_u64(
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
        }
    }
}

impl HttpConfig {
    /// Upper bound for all interruptible work in one retry sequence. DNS may
    /// exceed ureq's deadline on platforms where resolver calls cannot cancel.
    pub fn maximum_elapsed(&self) -> Duration {
        let attempts = u32::saturating_add(self.retries, 1);
        let dns_budget = self.timeout;
        let request_budget = self.timeout.saturating_mul(attempts);
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
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_bounded_u64(key: &str, default: u64, minimum: u64, maximum: u64) -> u64 {
    env_u64(key, default).clamp(minimum, maximum)
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
    post_json_with_cancel(cfg, url, api_key, body, || false)
}

pub fn post_json_with_cancel<F>(
    cfg: &HttpConfig,
    url: &str,
    api_key: &str,
    body: &Value,
    should_cancel: F,
) -> Result<Value>
where
    F: Fn() -> bool,
{
    if should_cancel() {
        return Err(MindLeakError::Cancelled(
            "optional HTTP request cancelled".to_string(),
        ));
    }
    if !breaker_allows(cfg, url) {
        tracing::warn!(target: "mindleak::net", %url, "circuit open; fast-failing without a request");
        return Err(MindLeakError::Http(format!("circuit open for {url}")));
    }

    let (expected_netloc, resolved_addresses) = resolve_endpoint(url, cfg.timeout)?;
    let agent = ureq::builder()
        .timeout(cfg.timeout)
        .timeout_connect(cfg.timeout)
        .timeout_read(cfg.timeout)
        .resolver(move |requested: &str| {
            if requested == expected_netloc {
                Ok(resolved_addresses.clone())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "redirect host is not pre-resolved",
                ))
            }
        })
        .build();

    let mut attempt = 0u32;
    loop {
        if should_cancel() {
            return Err(MindLeakError::Cancelled(
                "optional HTTP request cancelled".to_string(),
            ));
        }
        attempt += 1;
        let mut req = agent.post(url);
        if !api_key.is_empty() {
            req = req.set("Authorization", &format!("Bearer {api_key}"));
        }
        match req.send_json(body) {
            Ok(resp) => {
                let value = read_bounded_json(resp.into_reader())?;
                record_success(url);
                return Ok(value);
            }
            Err(err) => {
                let transient = is_transient(&err);
                tracing::warn!(target: "mindleak::net", %url, attempt, transient, error = %err, "http attempt failed");
                if transient && attempt <= cfg.retries {
                    if should_cancel() {
                        return Err(MindLeakError::Cancelled(
                            "optional HTTP request cancelled".to_string(),
                        ));
                    }
                    std::thread::sleep(backoff(attempt));
                    continue;
                }
                record_failure(url, cfg);
                return Err(MindLeakError::Http(err.to_string()));
            }
        }
    }
}

fn read_bounded_json(reader: impl Read) -> Result<Value> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(MindLeakError::Http(format!(
            "optional HTTP response exceeded {MAX_RESPONSE_BYTES} bytes"
        )));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn resolve_endpoint(url: &str, timeout: Duration) -> Result<(String, Vec<SocketAddr>)> {
    let parsed = url::Url::parse(url)
        .map_err(|error| MindLeakError::Http(format!("invalid URL {url}: {error}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| MindLeakError::Http(format!("URL has no host: {url}")))?
        .to_string();
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| MindLeakError::Http(format!("URL has no known port: {url}")))?;
    let netloc = format!("{host}:{port}");
    if let Ok(address) = host.parse::<IpAddr>() {
        return Ok((netloc, vec![SocketAddr::new(address, port)]));
    }

    let (sender, receiver) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("mindleak-dns".to_string())
        .spawn(move || {
            let result = (host.as_str(), port)
                .to_socket_addrs()
                .map(|addresses| addresses.collect::<Vec<_>>());
            let _ = sender.send(result);
        })?;
    match receiver.recv_timeout(timeout) {
        Ok(Ok(addresses)) if !addresses.is_empty() => Ok((netloc, addresses)),
        Ok(Ok(_)) => Err(MindLeakError::Http(format!(
            "DNS returned no addresses for {netloc}"
        ))),
        Ok(Err(error)) => Err(MindLeakError::Http(format!(
            "DNS resolution failed for {netloc}: {error}"
        ))),
        Err(RecvTimeoutError::Timeout) => Err(MindLeakError::Http(format!(
            "DNS resolution timed out for {netloc}"
        ))),
        Err(RecvTimeoutError::Disconnected) => Err(MindLeakError::Http(format!(
            "DNS resolver stopped for {netloc}"
        ))),
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
    fn cancelled_request_short_circuits_before_network_access() {
        let error = post_json_with_cancel(
            &HttpConfig::default(),
            "http://127.0.0.1:1/v1/test",
            "",
            &serde_json::json!({}),
            || true,
        )
        .unwrap_err();

        assert!(matches!(error, MindLeakError::Cancelled(_)));
    }

    #[test]
    fn maximum_elapsed_includes_all_attempts_and_backoff() {
        let config = HttpConfig {
            timeout: Duration::from_secs(10),
            retries: 2,
            breaker_threshold: 5,
            breaker_cooldown: Duration::from_secs(30),
        };

        assert_eq!(config.maximum_elapsed(), Duration::from_millis(40_300));
    }

    #[test]
    fn response_json_is_size_bounded() {
        let parsed = read_bounded_json(std::io::Cursor::new(br#"{"ok":true}"#)).unwrap();
        assert_eq!(parsed["ok"], true);

        let oversized = vec![b' '; MAX_RESPONSE_BYTES + 1];
        let error = read_bounded_json(std::io::Cursor::new(oversized)).unwrap_err();
        assert!(error.to_string().contains("exceeded"));
    }

    #[test]
    fn numeric_endpoint_resolution_is_immediate_and_exact() {
        let (netloc, addresses) =
            resolve_endpoint("http://127.0.0.1:11434/v1", Duration::from_millis(1)).unwrap();
        assert_eq!(netloc, "127.0.0.1:11434");
        assert_eq!(addresses, vec!["127.0.0.1:11434".parse().unwrap()]);
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
