- **The Bridge's Work read capability list still claims every command is
  `authorization_unavailable`, which is now stale — MEASURED 2026-08-30,
  OPEN.** ADR-0142 makes `work_command_api/`'s `submit`/`confirm` routes
  actually execute under the Bridge's hardened loopback profile (a real
  verified principal for a self-hosted, single-tenant deployment), but
  `crates/ackplane-bridge/src/work_api.rs`'s `command_capabilities()` -- the
  static, read-only list the Work page renders as ten disabled controls with
  an "authorization unavailable" reason, and that
  `tests/work_api_integration.rs` asserts unconditionally for all ten
  operations -- was deliberately left untouched by this change to keep the
  slice to the actual command routes ADR-0142 is about, not ADR-0120's
  separate read surface.
  The result: the Work page's UI now under-reports what the command routes
  underneath it can do. A caller reading `POST .../work/commands` directly
  sees a real `pending_confirmation` outcome; a caller reading the Work list
  response or looking at the rendered page still sees every command marked
  unavailable, which is the more alarming, definitely-wrong of the two.
  **What is actually needed:** `command_capabilities()` (and the Work page's
  rendering of it) needs its own reviewed pass deciding what an
  always-verified, no-adopted-policy loopback principal should report as
  `state` for a command list -- likely something other than a bare
  boolean, since `CreateWork`'s existing exception for a verified policy
  classifying it as routine (ADR-0125 decision 8) still has no policy layer
  to check per ADR-0142 clause 5. Closing this is scoped to `work_api.rs` and
  its test/HTML surface, not `work_command_api/` again.
