- **An advisory nudge now says that it is the reason.** Learned knowledge may
  attach an advisory and move an otherwise-aligned verdict to `needs_human` so a
  person looks — ADR-0022 §4, deliberately bounded so a decaying regularity can
  never hard-fail correct work. But the nudge changed the verdict and left only
  lines labelled `advisory:`, which read as information rather than as the
  cause. Every other route to `needs_human` pushes a finding naming its own
  reason; this one did not, so a receipt whose every other signal was positive —
  coverage confirmed, provenance intact, no drift, no lapse — reported an
  inexplicable failure, and the honest reading was unavailable to whoever held
  it. The nudge now records itself, naming the knowledge ids responsible and
  stating that nothing else in the evidence is a problem signal. The rule is
  unchanged; only its silence is fixed. A verdict knowledge did **not** move
  still carries no nudge line, so a drift is never blamed on an advisory.
