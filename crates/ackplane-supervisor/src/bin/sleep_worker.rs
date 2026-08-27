//! Test fixture only: not part of the public crate surface. Sleeps for a
//! caller-specified duration in milliseconds (default 30 seconds) so
//! `worker_adapter` tests can observe a real, deterministic, cross-platform
//! child process without shell-specific plumbing (AGENTS.md toolchain
//! discipline).

fn main() {
    let millis: u64 = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(30_000);
    std::thread::sleep(std::time::Duration::from_millis(millis));
}
