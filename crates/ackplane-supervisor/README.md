# ackplane-supervisor

The enrolled, runtime-neutral agent runner (ADR-0116). One process can run up
to 32 configured worker slots concurrently. Slots may use the same agent CLI
or different runtimes; each has its own workspace, branch, session, lease,
context packet, and durable receipts.

## Start Agents

1. Build the server and supervisor from the same revision. An older running
   container does not acquire the new context protocol from a source edit.
2. Follow the [Industrial quickstart](../../docs/INDUSTRIAL-QUICKSTART.md) for
   the server, TLS trust, and enrolled node identity. Configure the environment
  variables below and keep `register-me serve --state-dir ABSOLUTE_STATE_DIR`
  running with the provider used for enrollment. See
  [provider-backed enrollment](../../DEVELOPERS.md#provider-backed-enrollment).
  Publish the repository's adopted constitution and Work
   tasks, including goals, acceptance criteria, and declared file scope.
3. Install and authenticate each agent executable. Give each slot a separate
   checkout or worktree, and declare its actual branch. The runner does not
   create branches or grant an agent additional tool permissions.
4. Provide a JSON file such as `workers.json`, replacing the example absolute
   paths with existing directories on the worker host:

```json
{
  "copilot-a": {
    "command": "copilot",
    "args": ["-p", "{prompt}"],
    "working_directory": "/absolute/path/to/checkout-a",
    "branch": "agents/copilot-a"
  },
  "claude-b": {
    "command": "claude",
    "args": ["-p", "{prompt}"],
    "working_directory": "/absolute/path/to/checkout-b",
    "branch": "agents/claude-b"
  }
}
```

These are executable/argument examples, not built-in vendor integrations.
Configure authentication and allowed tools for your installed CLI version.
Any non-interactive executable accepting a prompt argument can use the same
adapter. The standalone `{prompt}` argument is replaced as one argument, never
evaluated by a shell. Control-plane environment variables are stripped from
children; ordinary runtime/provider settings are retained.

```text
cargo run --locked -p ackplane-supervisor --bin ackplane-supervisor -- --workers workers.json
```

The Bridge's Supervisors page shows each slot's registered session. Use the
existing authorized Work command flow to assign a published task to its node
and session. Each assignment must be confirmed before delivery. The supervisor
obtains the authoritative task lease, requests a scoped context packet, checks
its digest and freshness, and starts the configured executable. An `applied`
receipt moves Work to `claimed`; an exit code does not complete the task.

Work creation accepts `declared_paths` and `declared_symbols` through the Bridge
API. With `ackplane-workctl`, repeat `--path` and `--symbol` on both submission
and confirmation. Changing that scope changes the confirmation digest.

With no worker definitions the daemon remains notification-only. Configured
workers advertise `notify`, `assign`, and force termination. Generic processes
do **not** advertise live prompt injection, steer, pause, resume, drain, or
checkpoint support. Unsupported operations are refused, not approximated.

## Memory And Guardrails

Ackplane reserves the complete mandatory envelope before optional context:
identity, task lease and scope, objective, acceptance, constitution, policy,
safety controls, and required evidence. It refuses missing authority or an
insufficient budget. Optional context contains active, decay-ranked lessons,
bounded graph relationships, and recent observed outcomes from previous
sessions of the same task. Missing graph projection is stated explicitly.

Every packet is stored with its digest and source references. A recorded lesson
remains a candidate until activated by the knowledge workflow. Activated lessons
can change the next prompt, and a prior failed worker can inform a retry, but
neither becomes policy or proof of success. No model call is required to compile
this context; this is evidence-informed guidance, not model-weight training.

Native worker processes are **not a sandbox**. Workspace separation, scoped
leases, authenticated directives, process-group ownership, and replay checks
are enforced; prompts alone cannot prevent an agent from reading other files or
using its user's permissions. Use an isolated account/container runtime when
hard filesystem or network restrictions are required. Completion still needs
the existing evidence/conformance or human-review workflow.

## Configuration

The companion owns identity, credential access, TLS and outbound connections.
The supervisor receives only its local directory and expected repository scope.

| Variable | Required | Meaning |
|---|---|---|
| `MINDLEAK_ACKPLANE_STATE_DIR` | yes | Absolute state directory of the running node companion |
| `MINDLEAK_ACKPLANE_TENANT_ID` | yes | Enrolled tenant |
| `MINDLEAK_ACKPLANE_REPOSITORY_ID` | yes | Enrolled repository |
| `ACKPLANE_SUPERVISOR_ID` | yes | 1-64 ASCII letters, digits, hyphens or underscores; one node may run several |
| `ACKPLANE_SUPERVISOR_STATE_DIR` | no | Durable inbox/outbox directory (default `.mindleak/supervisor`) |
| `ACKPLANE_SUPERVISOR_HEARTBEAT_SECONDS` | no | Heartbeat interval (default `30`) |
| `ACKPLANE_SUPERVISOR_WORKERS` | no | JSON worker map when not using `--workers`; the file argument takes precedence |
| `RUST_LOG` | no | Log filter (default `info`) |

A missing variable is refused at startup, and **every** missing one is named at
once — configuring a new node otherwise means learning about the next omission
only after fixing the previous one.

Legacy `_NODE_ID`, `_SIGNING_KEY_ID`, `_NODE_SIGNING_KEY_SEED` and
`MINDLEAK_ACKPLANE_KEY_PATH` overrides are refused. Do not export the provider's
key or create another credential entry for the supervisor. The companion returns
public identity and constructs each signed domain request itself.

Each supervisor process needs its own state directory. Startup holds an
exclusive SQLite lock in `ownership.db` before checking recovery markers or
connecting to Ackplane. This prevents two processes from registering over the
same durable directory while idle; configured slots within one process still
run concurrently. Normal exit or process termination releases the lock without
deleting its file. Never delete or replace that file while a supervisor is
running. A released ownership lock does not prove a previously active worker
stopped: existing worker-run markers still require recovery. This local guard
does not compare workspaces configured under different state directories;
continue to give every slot a separate checkout or worktree.

## When it stops

- **Node companion unavailable or authority refused** - stops owned workers,
  queues termination evidence and retains unconfirmed lease/outbox state for
  recovery. It does not continue execution in an endless reconnect loop after
  losing its identity owner. A companion authority check failure also closes
  existing streams; restoring a credential is not permission to discard worker
  recovery markers.
- **State directory already in use** - refuses before registering or opening
  session queues. Stop the current owner or choose a separate state directory;
  removing its lock file is not a recovery operation.
- **Ctrl+C or SIGTERM on Unix** - stops every configured slot, terminates owned
  processes, releases active leases and flushes durable receipts. Shutdown has
  a thirty-second deadline. Transient disconnects during the flush reconnect
  and replay retained receipts within that same window; retries never reset
  the deadline. Exhausting the window exits unsuccessfully and retains the
  run marker and queue evidence for recovery. Permanent rejection or
  unrecoverable evidence stops the flush immediately without deleting it.
  Confirmed leases are tracked before context preparation, so stopping while
  awaiting a context reply also releases them without starting a worker or
  reporting a lifecycle for a process that never ran. A failed preparation
  release remains tracked and is retried between directives while the daemon
  is running. Requests whose grant was never confirmed and abrupt process loss
  remain subject to the server lease expiry and operator recovery boundaries.
- **Connection dropped or acknowledgement stalled** - reconnects after a short
  delay. Acknowledgement waits are bounded to ten seconds. Active leases are
  renewed; failed renewal stops the owned worker rather than inventing authority.
  Frames accepted by the server whose acknowledgements were lost remain in the
  outbox and are replayed idempotently, including during shutdown.
- **Queued frame permanently rejected** - stops delivery with the rejected
  sequence, server reason and diagnostic. The rejected frame and all later
  frames stay in the durable outbox; the acknowledged position advances only
  through the accepted prefix. Preserve the outbox and any worker-run marker
  for operator recovery. A refusal is not an acknowledgement, and restarting
  does not repair invalid evidence. Retryable refusals retain the same bytes
  and follow the existing reconnect path. Fatal cleanup queues its terminal
  receipt locally without attempting delivery past the rejection.
- **Stored frame cannot be replayed identically** - pending, archived and
  recovery reads refuse valid protobuf encodings whose decode/re-encode cycle
  changes the original bytes, including unknown fields from a newer version.
  The error names the sequence. The sender stops before transmitting its loaded
  batch; stored bytes and acknowledgement positions remain unchanged. Preserve
  the queues and use a version that supports their encoding. Do not strip fields,
  rewrite the record or delete evidence to make an older reader accept it.
- **One worker slot fails** - signals the remaining slots before attempting its
  own cleanup. The failed slot stops its owned worker and retries confirmed
  lease release for up to thirty seconds, preserving its run marker and queued
  evidence. Peers use their normal process-stop, lease-release and receipt-flush
  path, with up to thirty seconds to finish. The supervisor still exits
  unsuccessfully with the original error; additional cleanup errors are logged.
  Unaccepted evidence and unfinished cleanup remain for operator recovery. This
  does not automatically restart failed work or repair invalid evidence.
- **Unaccounted previous run** - refuses to reuse that worker slot. Its
  `<slot>.worker-run.json` marker identifies the session, workspace and durable
  queue files. Preserve that evidence and use the stopped-run recovery command
  below. Deleting the marker is not proof the old worker stopped. Runs without
  positive stop provenance still require investigation; automatic recovery of
  live or unproven workers is not implemented.
- **Server position outside the recoverable outbox interval** - stops
  deliberately. A position beyond the last locally enqueued frame means local
  evidence was lost; a position below the acknowledged boundary means the server
  needs frames already pruned locally. Retained frames alone cannot repair either
  gap, so the daemon preserves state for operator recovery.

Normal worker completion cleans up its owned process group before a terminal
lifecycle is reported. A parent process exiting does not permit a remaining
descendant to continue writing after the task lease is released.

On macOS, signalling a group whose leader exited but has not been reaped can
return `EPERM`. On a Unix permission error, cleanup checks the leader with a
nonblocking wait and retries the group signal once only if that wait confirms
the leader exited. It still signals any surviving descendants. A permission
error while the leader is live, or from the retry, remains a cleanup failure;
it is never treated as proof that the group is gone. Diagnostics distinguish
spawn, stop and wait failures, which remain failed, non-replayable effects.

Successful spawn is tracked before the inbox records its final effect or the
outbox queues context-use and startup receipts. Failure at any of those writes
therefore still reaches explicit worker termination. Active cleanup state is
cleared only after terminal evidence is durably queued. If that write also
fails, the lease is not released and the run marker remains for operator
recovery; the server's existing lease expiry still applies. Cleanup neither
fabricates the missing receipts nor treats an uncertain effect as a new spawn.

The slot retains its session and run marker until its lease release is
confirmed and all durable receipts are acknowledged. A release RPC failure
leaves cleanup pending and is retried; a terminal worker is not announced as
started again while waiting. A successful owner-guarded release no-op also
confirms there is no live claim to give back. Shutdown keeps its thirty-second
deadline and preserves existing recovery markers if cleanup cannot finish.

Acknowledgement retains the exact encoded lifecycle receipts and their original
outbox sequences in `acknowledged_lifecycle_receipts`, in the same transaction
that removes their pending delivery copies. Archive failure or corrupt pending
bytes rolls back acknowledgement. Other frame types are pruned as before, and
archived receipts never become pending again or change the delivery positions.
The library's `acknowledged_lifecycle_receipts(after_sequence, limit)` read
returns pages in sequence order, capped at 100 records; start with cursor zero
and continue after the last returned sequence. Records remain in that session's
outbox database and remain protected by its original identity binding.

The archive alone does not establish process termination, grant authority, or
certify task completion. Upgrading preserves lifecycle frames still pending
when they are acknowledged, but cannot reconstruct older pruned receipts.
Missing history remains unknown.

Use `SupervisorOutbox::open_read_only(path, registration, session)` for library
inspection of an existing outbox. It validates the stored identity/session without
creating directories, initializing identity, changing journal mode or migrating
schema. SQLite read-only access also refuses enqueue and acknowledgement through
that handle. Normal WAL reads remain enabled so inspection includes committed
records still in the log; it does not use immutable-file mode. Missing or corrupt
databases are refused. An older outbox can expose its existing pending frames and
positions, but reading an absent lifecycle archive reports a schema error rather
than creating an empty history. No process control or lease operation is performed.

## Recover A Stopped Run

This command finishes interrupted cleanup for a worker that the original
supervisor positively stopped. It never starts, adopts, or signals a process.
Keep the original worker configuration, supervisor ID, state directory and
repository scope. Stop the old supervisor first; keep the enrolled node
companion running for confirmation.

```text
ackplane-supervisor --workers workers.json recover inspect copilot-a
```

Inspection is local and read-only. Its JSON includes the recorded node,
session, worker, task, workspace and declared branch, `stopped`, the outbox
positions, `run_id`, and `confirmation_digest`. It does not need a running
companion. Review the target and use the returned run ID and confirmation
digest in the explicit cleanup command:

```text
ackplane-supervisor --workers workers.json recover confirm copilot-a RUN_ID CONFIRMATION_DIGEST --reason "Receipt delivery failed during shutdown"
```

Confirmation holds the same exclusive state-directory lock as the daemon and
rechecks the preview before acting. It requires all of the following:

- A version-1 marker with the original registration, session, directive,
  context packet, queue locations, workspace and declared branch.
- A positive adapter-stop record bound to the exact marker bytes and committed
  atomically with its terminal receipt. A `completed`, `failed` or `terminated`
  label alone, a dead supervisor, PID absence, or expired lease is insufficient.
- Consistent identity-bound inbox/outbox evidence. Marker and queue files must
  be regular files in their derived state-directory locations. Inspection is
  bounded to a 64 KiB marker, 1 MiB original effect, and 1024 retained frames
  totalling at most 4 MiB. Unknown wire fields that cannot be replayed
  identically are refused rather than silently discarded.
- The original node companion and an independent server receipt position
  within the retained outbox interval. The handshake's echoed position is not
  accepted as this proof; omitted or inconsistent positions stop cleanup.

The command registers the historical supervisor scope, replays only its exact
pending frames, and releases only the recorded task/session owner. It does not
announce the worker as started, dispatch an old directive, acquire or renew a
claim, complete Work, or release a replacement owner's lease. A confirmed
release no-op is recorded as such. Remote operations share a thirty-second
deadline; disconnects and permanent refusals retain pending evidence and the
marker. JSON results go to stdout, diagnostics to stderr.

`worker_recovery_events` retains each confirmed snapshot's original reason
and start time, then its completed result, final receipt position and release
outcome. Completion is durable before removal of the matching marker. If the
response is lost, retry the same run ID and confirmation digest: a completed
audit resolves the result even after marker removal, but never removes a
replacement marker. If evidence progressed during an incomplete attempt,
inspect again and explicitly confirm its new digest.

Old or partial markers and missing stop records remain refused; the command
does not backfill proof from old lifecycle labels or repair inconsistent
history. Preserve those files for investigation. After successful cleanup,
restart the supervisor normally and issue a separately authorized assignment.
The local lock coordinates cooperating processes, not hostile same-user file
replacement, and the native worker sandbox boundary remains unchanged.

## Verify The Loop

Set `ACKPLANE_TEST_DATABASE_URL` to the isolated `ackplane_test` database, never
the live service database, then run:

```text
cargo test --locked -p ackplane-supervisor --test multi_agent_end_to_end
cargo test --locked -p ackplane-server --lib context_service::tests
```

The first test starts a real authenticated gRPC server, a protected companion,
and the supervisor binary without a private-key environment variable
with two concurrent fixture executables, checks their distinct memory-informed
prompts, credential separation, Work transitions, durable outcome receipts, and
orderly shutdown of two active workers on Unix. It also holds both context
replies after the real server has compiled them and verifies that shutdown
releases the confirmed leases without spawning workers or inventing lifecycle
receipts. Injected release RPC failures verify that both normal completion and
shutdown retry before clearing run markers, without duplicate lifecycle receipts.
Shutdown transport cases lose both terminal acknowledgements after the server
has accepted them, then verify clean idempotent replay, fixed-deadline exhaustion
under repeated disconnects, and retained evidence on permanent rejection.
An injected failure after both workers start verifies that a healthy peer
finishes shutdown before the supervisor reports the original error, while the
failed slot releases its own lease and retains its recovery marker. The same
case with transient release failures verifies retries without delaying the
peer's stop signal. Reopening the failed outbox verifies its original queued
frames remain unchanged and unacknowledged, followed by one unsent terminal
receipt. Temporary local queue triggers also reject context-use, startup,
applied-effect and terminal writes. Those cases verify the surviving receipt
prefix, retained uncertainty and lease state, including a cleanup retry after
terminal persistence fails.
An additional companion-loss case verifies worker termination and retained
unconfirmed cleanup evidence when local custody disappears.
The Unix recovery cases kill the supervisor after two real workers stop,
then exercise the CLI through the authenticated companion. They lose replies,
kill recovery after server acceptance, reject completion-audit writes, and
check omitted or out-of-range server positions, permanent refusal, unchanged
queued bytes, one terminal server history, replacement-owner protection, and
exactly one fresh separately authorized execution. A separate crash while both
workers are live verifies refusal without signalling or releasing them.
The second tests graph input, lesson activation, retry feedback and refusal of
cross-session or unleased requests. Without the database variable these gated
tests skip; a skipped run is not verification. The fixtures test the runtime
contract, not a live vendor model or its login/tool-permission configuration.

`cargo test --locked -p ackplane-supervisor --lib recovery::` needs no server.
It checks atomic stop/frame rollback, retained stop proof after acknowledgement,
immutable attempts, changed-preview refusal, old/corrupt/oversized evidence,
path confinement, state-lock exclusion, and crash retries around marker removal.

`cargo test --locked -p ackplane-supervisor --test instance_ownership` needs no
database or live server. It launches real supervisor processes to verify
duplicate startup refusal, idle process-death restart and independent state
directory concurrency.

`cargo test --locked -p ackplane-supervisor --test outbox_rejection` uses the
isolated database to verify real authenticated permanent and retryable server
refusals, the accepted position, and byte-preserving outbox reopen. It confirms
that a rejected frame cannot be skipped to transmit later evidence, and that
lossy decoding is refused locally before sending any frame in the loaded batch.

`cargo test --locked -p ackplane-supervisor --test outbox` also needs no server.
It verifies exact lifecycle retention across acknowledgement and reopen, bounded
cursor paging, identity isolation, rollback on an injected archive failure,
corrupt-frame preservation, and honest handling of older outboxes. Its read-only
tests also check mutation refusal, current WAL visibility, missing/corrupt files,
unchanged old schema and journal mode, and refusal to adopt a missing identity.
Pending and archived reads reject unknown wire fields without changing their
bytes or positions, through both writable and read-only handles.

`cargo test --locked -p ackplane-supervisor --lib worker_adapter::tests`
includes a macOS/Linux regression that waits for a real child's exit without
reaping it, then checks cleanup and stable terminal status for both unreaped
and already-reaped groups. It uses the safe `rustix` test API rather than a
fixed delay.

`cargo test --locked -p ackplane-supervisor --test worker_adapter` verifies
running-to-completed state using the existing TCP fixture: wait for readiness,
observe the held worker, close its connection to release it, and observe exit
within a bounded deadline. It does not infer completion from a fixed sleep.
The same readiness and exit helpers support the descendant-cleanup regression;
adapter ownership still cleans up child processes if an assertion fails.

### Live Agent Check

The same path was exercised on 2026-09-10 with two authenticated Copilot CLI
1.0.83 processes, using separate workspaces and permissions limited to their
own `result.json` files. Both first-round outputs matched their task/session/
packet identities, and both fresh second-round sessions received the first
round's packet ids and outcomes in their newly compiled prompts. The Bridge
recorded concurrent starts and completed worker lifecycles for both rounds.
Those observations verify the control-and-memory loop, not unrestricted coding
permissions, sandboxing, or automatic approval of an agent's work.

For an isolated reproduction, enroll `demo-agent-runtime` and configure its
enrolled identity, then publish its bounded demonstration policy:

```text
cargo run --locked -p ackplane-client --example publish_demo_constitution
```

This example uses signed gRPC, requires the exact demo repository name, and
refuses to overwrite an existing constitution. It does not access the database
or generate enrollment credentials. Use normal authorized Work commands to
create and assign tasks after configuring the worker executables.
