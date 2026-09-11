- `register-me activate` now atomically saves the assigned signing key ID and
  enrollment receipt in its existing sidecar before attempting synchronization.
  Restarted activation reuses that public record while NodeSync still verifies
  current authority. Repeated requests cannot overwrite saved enrollment, and
  the standalone `--skip-sync` flag reports recorded activation without claiming
  that the node is currently live or authorized.
