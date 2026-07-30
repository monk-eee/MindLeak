### Fixed

- **The pre-push hook no longer contaminates the suites it runs.**
  `canonical-push` sets `MINDLEAK_CANONICAL_PUBLISH` while running the pre-push
  hooks, and the extension-test runner passed the environment straight through.
  So the suite asserting that a *direct* invocation of the publisher is refused
  inherited the flag, saw the direct call allowed, and failed — while passing
  when run by hand.

  A runner whose answer depends on who invoked it is worse than one that does
  not run at all: it makes a real guard look broken, and sends the author
  chasing their own tooling instead of the bug. Caught when the hook blocked its
  own author's push on a suite that passed standalone.

  `MINDLEAK_CANONICAL_PUBLISH` and `PRE_COMMIT_REMOTE_BRANCH` are now scrubbed
  alongside git's `GIT_DIR` family, and the scrub is one exported function with
  its own tests rather than a list copied inline — including that unrelated
  variables survive and the caller's own environment is not mutated.
