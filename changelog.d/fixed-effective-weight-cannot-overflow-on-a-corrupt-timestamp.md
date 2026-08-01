- The decay weight function can no longer be crashed by a corrupt timestamp.
  `effective_weight` — evaluated on every edge in every graph query via its SQL
  scalar — computed `now - updated_at` as a raw `i64` subtraction, which panics
  on overflow in debug builds for an extreme or corrupt `updated_at`. It now uses
  `saturating_sub`, so the function is total: a pathological timestamp degrades to
  a fully-decayed edge instead of taking down the query. The `2^x` term is now
  computed with `exp2` for accuracy. Behaviour is unchanged for every realistic
  timestamp. A dead, uncalled `signal_half_life` helper was also removed —
  half-life graduation is applied through `signal_multiplier` in the promotion
  path, so the helper was misleading dead surface.
