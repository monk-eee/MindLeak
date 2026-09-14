# Industrial Regression Endurance

Run the existing Industrial qualification gate repeatedly against one committed
source candidate. This measures repeatability under sustained regression work,
not continuous service uptime, a load benchmark, model quality, or the
seven-day pilot in the [stabilization plan](INDUSTRIAL-STABILIZATION.md).

## Prerequisites

- Use a dedicated, clean worktree. Do not edit, switch, update or merge its
  source while the run is active.
- Provide disposable loopback PostgreSQL with pgvector, a primary database
  named `ackplane_test`, and a separate database ending in `_rehearsal` on the
  same instance. Set `ACKPLANE_TEST_DATABASE_URL` and
  `ACKPLANE_TEST_REHEARSAL_DATABASE_URL` accordingly. These databases are written
  and restored repeatedly; never reuse live data. The required names and
  confirmation flag are safeguards, not proof that a database is disposable.
- Install the prerequisites of `scripts/industrial-test.mjs`: Cargo, `pg_dump`,
  `pg_restore`, and a working native credential facility. Preserve the normal
  OS user profile for credential tests and isolate application/test state.
- Keep the machine running. The run needs disk space for its own build output
  and accumulating isolated test records. It does not remove those records or
  change the running application deployment.

## Run

```text
node scripts/industrial-soak.mjs --hours 8 --disposable-databases
```

The default is eight hours. A positive fractional `--hours` value permits a
short rehearsal; every successful run still requires at least two complete
cycles. A short rehearsal must never be reported as an eight-hour result.

Each cycle invokes the unchanged Industrial gate: recovery-tool checks,
all-feature/all-target workspace compilation, migration, and the complete
workspace test run with database, recovery and native-credential gates enabled.
Existing explicitly ignored model tests remain ignored. This does not replace
CI, the offline extension gate, or an installed-artifact pilot.

Build output and a private JSON-lines journal live in a unique directory under
`target/industrial-soak`. The runner prints its journal path at startup and
overrides `CARGO_TARGET_DIR` to keep other workstreams from replacing its build.
The journal records the commit/tree, requested duration, cycle boundaries,
command arguments and outcomes, monotonic elapsed time and final verdict.
It never serializes database URLs, environment contents or command output.
Normal test output stays on the terminal; retain the failing assertion when
investigating an incident.

## Interpret the Result

- Only a terminal `finished` event with `status: passed` proves the requested
  measurement completed. Its `measuredMs` is the sum of completed successful
  gate cycles, while `elapsedMs` includes surrounding setup and bookkeeping.
  Finishing the current cycle can exceed the requested duration.
- A failed command stops the run immediately. A nonzero exit, thrown error,
  invalid clock or changed source cannot earn successful measurement time.
- Source commit, tree and tracked/untracked cleanliness are checked before
  and after every cycle. An edit invalidates the run; these checks are not a
  sandbox against a process that changes and restores files between checks.
- An interruption is not success. Ctrl+C reaches the foreground test processes;
  signal handling records interruption after the current synchronous command
  returns. A forced kill or machine failure may leave no final event. An
  unfinished journal is unknown/incomplete, never an implicit pass. The wrapper
  does not add a second process-killing mechanism to the existing test runner.
- Preserve failed and unfinished journals. A new invocation is a distinct run;
  it does not resume a previous duration or erase earlier incidents.

Report the exact candidate, requested and measured duration, completed cycles,
any failures, and the tested workload. Do not promote a regression endurance
result into a production-readiness or eight-hour availability claim.

## Runner Tests

```text
node --test scripts/industrial-soak.test.mjs scripts/industrial-test.test.mjs
```

These tests cover duration accounting, failure and interruption, journal
failure, fixture consent, source changes in a real Git repository, and the
existing Industrial gate's command sequence. Unit-test clocks are simulated;
only the actual runner journal measures elapsed qualification time.
