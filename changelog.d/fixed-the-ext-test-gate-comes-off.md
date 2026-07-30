### Fixed

- **The extension-test hook comes off pre-push.** It was added to catch a rename
  landing without its consumer, and its own comment warned that "a gate that
  intermittently blocks a push is worse than none — it teaches people to reach
  for `--no-verify`". That sentence turned out to describe the hook.

  Under fleet load vitest reports `[vitest-worker]: Timeout calling
  "onTaskUpdate"` and exits non-zero **with every test passing** — `14 passed, 1
  error` — and the runner cannot tell that from a real failure. It blocked
  pushes across the fleet on tests that had in fact passed. That is worse than
  the breakage it was added to catch: a missed rename fails one branch's CI,
  this stopped everyone.

  The capability is kept, not the gate. `make ext-test` and
  `node scripts/ext-test.mjs <changed files>` still run it, and CI still runs the
  full suite. It earns its way back to pre-push when it can distinguish a worker
  timeout from a failed assertion, and not before.
