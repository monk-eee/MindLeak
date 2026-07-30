- **A scoped task claim now carries the memory pre-flight agents were
  skipping.** Live telemetry showed ADR-0066's adoption gate had failed: five
  writing sessions made 1,033 attributed writes without a successful memory
  read or MindLeak `check_overlap` before the first write. A won Lodestar
  `task_claim(step = "claim")` now returns a structured `memory_preflight` for
  the exact claimed paths and symbols, naming MindLeak `check_overlap` and the
  requirement to call it before the first edit. The response remains advisory
  and explicitly does not claim the cross-plane read already ran; unscoped or
  lost claims remain quiet. Memory-habit telemetry now counts a successful
  `check_overlap` as the deterministic retrieval ADR-0066 made it.
