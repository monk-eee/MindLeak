- **Pinned Industrial installation qualification remains incomplete after a
  supervisor restart conflict.** The six-command archive and isolated deployment
  at `43a5d7027ef86ad89feb83f74e6bed4c7c0c3469` passed installation, TLS enrollment
  retries and initial MCP checks, but `ackplane-supervisor::daemon::WorkerRuntime`
  regenerated `started_at` under its saved session ID and `disconnected_on_error`
  retried Ackplane's permanent rejection. Fixed this run with durable session
  persistence, refusal classification and red/green regressions, including a real
  server restart test. The failed archive is not qualified: rebuild every artifact
  from one reviewed revision and repeat installed client refusals, companion and
  service restart, and exact test-credential cleanup. See
  [the checkpoint](../docs/INDUSTRIAL-STABILIZATION.md#pinned-installation-qualification-2026-09-14).
