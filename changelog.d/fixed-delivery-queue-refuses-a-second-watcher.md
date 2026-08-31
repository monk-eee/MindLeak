### Fixed

- `delivery-queue --watch` now refuses to start when another watcher has
  checked in within the last five minutes, instead of starting a second one
  alongside the first. Two watchers do not share the queue, they duplicate it:
  both read the same order, both pick the same head-of-queue branch, and both
  run `gh pr update-branch` on it — doubling the check runs that taking one
  turn at a time exists to avoid, and risking a bogus "tree does not match the
  expected merge" alarm when one watcher's expected tree is computed before the
  other's update lands. The heartbeat that makes this detectable already
  existed; `--watch` wrote one every tick and never read one. Pass `--force` to
  start anyway — a beat carries no identity, so a watcher that died leaves one
  that stays fresh for up to five minutes.
