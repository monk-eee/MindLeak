- **The Vitest run intermittently dies with `Timeout calling "onTaskUpdate"` —
  OPEN, local-only, two fixes attempted and reverted.** — Hit repeatedly on
  Windows during one session on a full `npm --prefix editors/vscode test`. It is
  a worker→reporter RPC timeout, not an assertion, so it reports as a run
  failure naming no failing test while **209/209 tests pass**. Cause: the
  `scripts/**` suites drive Git and Node through `execFileSync` / `spawnSync`,
  which block the worker's event loop for the whole child process, so the worker
  cannot answer the reporter's heartbeat.
  Tried and reverted, both still timing out: bounding that project to two worker
  threads, and moving it to `pool: "forks"` (which was also *slower* — 92s
  against 70s). A blanket `fileParallelism: false` does pass 209/209 but more
  than doubles the wall clock (114s against ~48s) to fix a problem only one
  group of files has. **CI on ubuntu does not hit this**, so it is a local
  annoyance rather than a pipeline risk — which is exactly why it must stay
  written down: it trains people to re-run instead of read, and one day it will
  mask something real.
  A real fix means making the Git fixtures cheaper (a template repository copied
  per test instead of `git init` plus commits), not more pool tuning.
