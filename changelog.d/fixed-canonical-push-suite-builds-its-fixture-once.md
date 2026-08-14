- The canonical-push test suite no longer rebuilds its git fixture for every
  test. It is built once and copied, which took the suite from 163.7s to about
  51s and the slowest test from 17.1s to 12.8s — most of a test's time was ten
  git subprocesses recreating the same repository rather than exercising the
  publisher. The per-test timeout is now derived from that measurement instead of
  being an undocumented number, which is what let the suite time out in CI and
  block two unrelated pull requests.
