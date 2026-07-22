//! Exponential half-life decay for edge weights.
//!
//! Effective weight `W_eff = W_base * 2^(-Δt_hours / half_life)`.
//! An edge whose effective weight falls below [`PRUNE_THRESHOLD`] is treated
//! as inactive at query time and is eligible for background purging.

/// Edges below this effective weight are ignored in queries and pruned.
pub const PRUNE_THRESHOLD: f64 = 0.05;

/// Compute the time-decayed effective weight of an edge.
///
/// * `base` — stored base weight (0.0..=1.0).
/// * `half_life_hours` — hours for the weight to halve.
/// * `updated_at` — unix seconds of last reinforcement.
/// * `now` — current unix seconds.
pub fn effective_weight(base: f64, half_life_hours: f64, updated_at: i64, now: i64) -> f64 {
    if half_life_hours <= 0.0 {
        return base;
    }
    let dt_hours = (now - updated_at) as f64 / 3600.0;
    if dt_hours <= 0.0 {
        return base;
    }
    base * 2f64.powf(-dt_hours / half_life_hours)
}

/// True if an edge is still considered active given its effective weight.
pub fn is_active(effective: f64) -> bool {
    effective >= PRUNE_THRESHOLD
}

/// Minimum reinforcements before an edge can earn a longer half-life.
pub const SIGNAL_MIN_COUNT: i64 = 3;
/// Minimum span (hours) the reinforcements must be spread across. Defeats
/// same-session spam: frequency alone is not signal (ADR-0005).
pub const SIGNAL_MIN_SPAN_HOURS: f64 = 48.0;
/// Cap on how far proven signal may stretch a half-life.
pub const SIGNAL_MAX_MULTIPLIER: f64 = 8.0;

/// Graduate an edge's half-life by *proven signal*: reinforcement corroborated
/// across time earns a longer half-life, so it resists decay. One-offs (low
/// count) and same-session spam (narrow span) keep the base half-life — "decay
/// noise, not signal" (ADR-0005). A pure function of edge fields; nothing stored.
pub fn signal_half_life(
    base_half_life: f64,
    reinforcement_count: i64,
    first_seen: i64,
    updated_at: i64,
) -> f64 {
    if reinforcement_count < SIGNAL_MIN_COUNT {
        return base_half_life;
    }
    let span_hours = (updated_at - first_seen) as f64 / 3600.0;
    if span_hours < SIGNAL_MIN_SPAN_HOURS {
        return base_half_life;
    }
    let multiplier = (1.0 + (reinforcement_count as f64).log2()).min(SIGNAL_MAX_MULTIPLIER);
    base_half_life * multiplier
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_edge_keeps_base_weight() {
        assert!((effective_weight(1.0, 24.0, 100, 100) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn halves_after_one_half_life() {
        // 24h half-life, 24h elapsed -> 0.5
        let now = 24 * 3600;
        assert!((effective_weight(1.0, 24.0, 0, now) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn decays_below_threshold_after_many_half_lives() {
        // 24h half-life, ~5 days elapsed -> well under 0.05
        let now = 5 * 24 * 3600;
        let w = effective_weight(1.0, 24.0, 0, now);
        assert!(w < PRUNE_THRESHOLD);
        assert!(!is_active(w));
    }

    #[test]
    fn intent_outlives_execution() {
        let now = 7 * 24 * 3600;
        let intent = effective_weight(1.0, 168.0, 0, now);
        let exec = effective_weight(1.0, 24.0, 0, now);
        assert!(intent > exec);
    }

    #[test]
    fn signal_half_life_ignores_one_offs_and_spam() {
        // Too few reinforcements -> base half-life.
        assert_eq!(signal_half_life(24.0, 1, 0, 1_000_000), 24.0);
        // Enough count but all within a narrow span (spam) -> base half-life.
        let narrow = (SIGNAL_MIN_SPAN_HOURS as i64 - 1) * 3600;
        assert_eq!(signal_half_life(24.0, 10, 0, narrow), 24.0);
    }

    #[test]
    fn signal_half_life_extends_for_corroborated_signal() {
        // 3 reinforcements across 100h -> longer half-life, capped.
        let span = 100 * 3600;
        let hl = signal_half_life(24.0, 3, 0, span);
        assert!(hl > 24.0);
        assert!(hl <= 24.0 * SIGNAL_MAX_MULTIPLIER);
    }
}
