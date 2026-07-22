//! Long-horizon revalidation decay for consolidated knowledge (ADR-0005).
//!
//! The same half-life formula as MindLeak's episodic decay, but knowledge uses a
//! long half-life and is *reconfirmed* by fresh evidence (`confirmed_at`) rather
//! than expiring fast. Kept as a local pure function so the Intent Plane stays
//! decoupled from the memory crate (ADR-0004): the shared thing is a
//! mathematical identity, not a behavioural helper to keep in sync.

/// Default half-life for learned knowledge (~30 days). Durable, not immortal.
pub const KNOWLEDGE_DEFAULT_HALF_LIFE_HOURS: f64 = 720.0;

/// Knowledge below this effective weight is inactive and eligible for pruning.
pub const ACTIVE_THRESHOLD: f64 = 0.05;

/// `W_eff = W_base · 2^(−Δt_hours / half_life)`.
pub fn effective_weight(base: f64, half_life_hours: f64, confirmed_at: i64, now: i64) -> f64 {
    if half_life_hours <= 0.0 {
        return base;
    }
    let dt_hours = (now - confirmed_at) as f64 / 3600.0;
    if dt_hours <= 0.0 {
        return base;
    }
    base * 2f64.powf(-dt_hours / half_life_hours)
}

/// True if knowledge is still considered active given its effective weight.
pub fn is_active(effective: f64) -> bool {
    effective >= ACTIVE_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_knowledge_keeps_base_weight() {
        assert!((effective_weight(1.0, 720.0, 100, 100) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn knowledge_outlives_episodic_decay() {
        // 7 days elapsed: 30-day knowledge half-life stays high; 24h episodic dies.
        let now = 7 * 24 * 3600;
        let knowledge = effective_weight(1.0, 720.0, 0, now);
        let episodic = effective_weight(1.0, 24.0, 0, now);
        assert!(knowledge > 0.8);
        assert!(episodic < ACTIVE_THRESHOLD);
    }

    #[test]
    fn unconfirmed_knowledge_eventually_fades() {
        // ~5 half-lives without reconfirmation -> below threshold (use it or lose it).
        let now = 5 * 720 * 3600;
        assert!(!is_active(effective_weight(1.0, 720.0, 0, now)));
    }
}
