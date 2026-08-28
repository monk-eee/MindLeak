- **Fixed:** `telemetry_snapshot` (`mindleak-core`/`mindleak-mcp`) no longer
  counts a refused call as a failed one. `reset_database` rejecting a request
  without its exact confirmation token is a guard doing its job, not the
  engine faulting, but it was recorded as `outcome='error'` indistinguishably
  from a genuine fault -- `telemetry_snapshot` reported a 100% lifetime
  failure rate for a tool that had never actually been broken, the same shape
  PR #781 already fixed for projection freshness. `outcome` gains a fourth
  value, `refused`, alongside `ok`/`error`/`skipped`; `NameMetric` gained a
  `refused: i64` lifetime count so the information is re-bucketed rather than
  dropped, and `currently_failing`/lifetime `errors` exclude it. See
  ADR-0010's amendment.
