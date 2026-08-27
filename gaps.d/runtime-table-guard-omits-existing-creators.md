- **The new lazy-table guard did not enumerate the runtime tables it claimed to
  protect -- VERIFIED 2026-08-27, repair in progress.** It invoked only the
  embedding builder, so `telemetry_events` in MindLeak and `model_call_events`
  in Lodestar remained outside the asserted set. A third table could likewise
  be added in another module without the test noticing, leaving the two-plane
  recurrence guard unable to prevent the foreign-key omission it advertised.
