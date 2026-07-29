- **`graph_multi_hop_query` is in a failing state and nobody noticed — OPEN.**
  `telemetry_snapshot` reports `currently_failing: true` for it, with a last
  error of `missing required argument: seed_entity`: a malformed call to the
  headline traversal capability, never followed by a successful one. It has 10
  lifetime calls. Impact: low today precisely because nothing depends on it,
  which is the actual finding — a tool with no callers has no failure signal
  either, so this could have been broken for any length of time. Left open
  deliberately: ADR-0066 predicts the read-to-write ratio should move, and if it
  does this tool starts mattering.
