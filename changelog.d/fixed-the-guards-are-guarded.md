- **The repository's own guards are now tested by CI instead of by nobody.**
  Six test files under `scripts/` — covering the conformance gate, the
  merge-driver guard, the claim gate, the publication record, the delivery
  queue and the board health report — carried a header saying `Run with: node
  --test scripts/` and were wired into nothing: no CI job, no Makefile target,
  no hook. Forty-five assertions about the machinery that decides whether work
  is honest ran only when somebody remembered to type the command, which is to
  say they had not run in a long time. Worse, the command in the header no
  longer works: passing a directory to `node --test` fails on Node 24, and the
  glob that replaces it fails on the Node 20 that CI pins, so a developer
  following the instruction got a module-resolution error rather than a test
  run. `make script-test` now enumerates the files and passes them explicitly,
  which works on both versions and on every OS, and it runs in CI, in `make ci`
  and on pre-push. The runner refuses to report success when it discovers no
  test files at all — a runner that quietly finds nothing is indistinguishable
  from a green suite.
