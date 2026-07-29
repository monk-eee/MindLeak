- **The release smoke reported success on platforms it never executed —
  FIXED.** — Found Jul 2026 diagnosing why v0.1.3 tagged but published nothing.
  The smoke compared `process.arch` against the target architecture and, on a
  mismatch, printed a notice and returned — so the job went green having tested
  nothing. `macos-x64` builds `x86_64-apple-darwin` on `macos-14`, which is
  arm64, so **two of the four platforms in every release since the step was
  added were never smoke-tested**. That is how a startup crash on
  `MINDLEAK_DB=":memory:"` — which killed the server outright on Linux and
  macOS — reached a tagged release with green ticks beside it. — High impact:
  the check most trusted to prove a binary runs was the one not running, and it
  said so only in a notice nobody reads. — Fixed this run: the x64 macOS build
  moved to `macos-15-intel` so every target is native, and an architecture
  mismatch is now `core.setFailed` rather than a skip. A binary the workflow
  cannot execute is one it must not ship, and a green tick that means "not
  checked" is worse than a red one. Note the Intel label matters: `macos-13` was
  tried first and is retired, and an unknown `runs-on` label does not fail — the
  job queues forever with no runner able to claim it.
