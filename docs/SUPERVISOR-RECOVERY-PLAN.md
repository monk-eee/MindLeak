# Supervisor Restart Recovery Plan

**Status: Proposed. No recovery command is implemented or authorized by this document.**

Baseline: `0ba768fbf9810d147937a8bbafc7cb150927ff17`, including merged
PRs #931 and #932. Implementation task: `task:442ec9fea970`, waiting for the
node-companion runtime handoff. The user chose to wait for that handoff; this
document does not release the wait or complete STAB-04.

This proposal applies the existing supervisor boundary in
[ADR-0116](adr/0116-enrolled-supervisors-are-the-distributed-agent-runtime.md)
and the [industrial stabilization criteria](INDUSTRIAL-STABILIZATION.md).
New process-ownership or authority decisions still require design review before
implementation. The current [process-loss gap](../gaps.d/industrial-worker-process-loss-needs-operator-recovery.md)
remains open.

## Verified Starting Point

| Existing surface | What it establishes | What it does not establish |
| --- | --- | --- |
| [WorkerRuntime](../crates/ackplane-supervisor/src/daemon/runtime.rs) | Writes a run marker before spawn; tracks a successful spawn before fallible receipt writes; stops the owned process before queuing a terminal receipt. | Its marker alone does not prove a spawn occurred or that an orphaned worker stopped. |
| [State-directory ownership](../crates/ackplane-supervisor/src/storage.rs) | A process-lifetime SQLite lock excludes cooperating supervisors using the same state directory. | A released lock does not prove the old workers stopped. |
| [Outbox](../crates/ackplane-supervisor/src/outbox.rs) | Preserves pending frames and original acknowledged lifecycle bytes/sequences; offers identity-bound read-only access. | Missing historical receipts cannot be reconstructed, and a caller-written lifecycle frame is not independent OS evidence. |
| [Receipt delivery](../crates/ackplane-supervisor/src/daemon/delivery.rs) | Replays original queued frames and acknowledges only accepted replies. | Reconnecting cannot repair a permanent refusal or absent local evidence. |
| [Reconciliation](../crates/ackplane-supervisor/src/reconcile.rs) | Compares the retained interval with the server's independently reported accepted position. | An absent server position is not zero and must not certify consistency. |
| [Claim release](../crates/ackplane-supervisor/src/daemon/claims.rs) | Uses the enrolled node and recorded session owner for the server's release operation. | Local records, elapsed lease time or transport success alone do not grant replacement authority. |

The current marker includes supervisor, session, worker, task and packet IDs,
workspace and queue paths. It does not persist the full original registration
or session metadata. `inbox_identity` binds tenant, repository, node, supervisor
and session, but not worker ID or task ID. Recovery must validate the remaining
bindings against their records; it cannot treat opening a matching outbox as
validation of the entire run.

## Proposed Scope

The first executable operation should finish cleanup of an **already-stopped,
fully accounted-for run**. It must not launch work, adopt a live process, or
invent a missing terminal report. Ordinary startup continues to refuse run
markers until that explicit cleanup succeeds.

If the supervisor died while its workers were active, missing or ambiguous stop
evidence means refusal. Solving that case requires an adapter-owned way to retain
process ownership across supervisor loss. It is a separate unresolved part of
this design, not a successful result of the narrower cleanup operation.

Neither a PID lookup nor a stored PID plus a timestamp is sufficient permission
to signal a process. Never infer termination from a dead supervisor, an expired
lease, an empty outbox, a closed connection, or an operator deleting a marker.
Never copy an old session onto a new worker execution.

## Inspection Contract

Inspection must be non-mutating and bounded. It reads existing files and normal
SQLite WAL state; it does not initialize schema, repair identity, checkpoint a
live database, prune receipts or contact the claim service. Separate reads of a
changing outbox are not a consistent snapshot: capture one database read
transaction, and treat concurrent marker changes as an invalidated observation.

Required validation before presenting a run as eligible for cleanup:

1. Resolve the operator-selected state directory and slot against local
   configuration. Decode a bounded marker with a supported version and complete
   identity. Reject malformed identifiers before deriving any queue pathname.
2. Derive queue locations beneath that state directory. Marker paths are
   assertions to compare, not arbitrary file locations to open. Refuse path
   traversal, symlink escape, substituted files, or workspace/configuration
   disagreement. Do not repair these mismatches by rewriting the marker.
3. Match tenant, repository, node, supervisor and session across the configured
   identity and both queues. Match worker, task, directive and packet references
   using their original records. Unknown fields needed for that binding are a
   refusal, not defaults copied from the current process.
4. Decode and validate original pending and archived lifecycle frames, including
   their embedded sequence, worker/session identity and protocol values. Reject
   conflicting terminal reports, contradictory later activity and incomplete
   scans. Archive pages are capped at 100; a bounded total scan that truncates
   must say so and cannot establish cleanup eligibility.
5. Require positive stop provenance tied to that exact run and a reviewed
   producer/adapter contract. `Completed`, `Terminated` or `Failed` text alone
   is insufficient. In particular, a generic `WorkerLost` report must not be
   interpreted as proof that every owned descendant was stopped.

Inspection should distinguish these results instead of returning a success-shaped
empty list:

| Result | Meaning | Permitted effect |
| --- | --- | --- |
| Live owner or worker | A cooperating runtime still owns the state or the run. | None. Ask that owner to stop through its normal path. |
| Stop unproven | The marker exists but sufficient positive stop provenance does not. | None. Preserve all records for operator investigation. |
| Evidence inconsistent | Identity, paths, sequence history or required schema cannot be reconciled. | None. Name the failed check without disclosing secrets. |
| Stopped, cleanup pending | Exact run has sufficient stop provenance; receipts or release still need confirmation. | Eligible for a separately confirmed cleanup attempt, not automatic execution. |
| Cleanup recorded | A completed cleanup receipt names the exact run and marker digest. | Read the existing result; never repeat worker execution. |

These names describe proposed outcomes, not shipped enums or API fields.

## Confirmed Cleanup Ordering

The operator confirms an inspection result for one run with a reason. Confirmation
binds the exact marker digest and original run identity. Its authorizing identity
and policy must use the handed-off node/provider contract, not a new fallback
signer or a freeform label treated as authentication.

1. Acquire the existing state-directory ownership guard for the whole mutation
   attempt. Revalidate the marker, paths, identity and stop evidence under that
   guard. A previously displayed preview is not permission to use changed files.
2. Record the cleanup attempt and reason durably before external mutations, using
   an idempotency identity bound to the run and preview. Do not overwrite the
   original execution or lifecycle evidence.
3. Authenticate through the current enrolled-node/companion boundary. Register
   only the metadata needed for historical receipt delivery; do not announce the
   old worker as `Started`, solicit assignments or run the ordinary dispatch loop.
4. Compare the independent server accepted position with the original outbox's
   acknowledged/high-water interval. Refuse unknown or out-of-interval state;
   never fabricate a zero or advance past evidence the queue cannot supply.
5. Replay original pending bytes with their original sequence numbers. Advance
   acknowledgement only on accepted replies. Retry transient failures within one
   fixed deadline; permanent refusal preserves the refused frame and its tail.
6. Release only the lease held by the recorded owner through the authoritative
   service. A replacement owner's claim must remain untouched. Interpret
   already-released, expired and ownership-mismatch responses using that service's
   actual contract, not a guessed boolean meaning. No claim acquisition or renewal
   belongs in recovery cleanup.
7. Durably record the confirmed receipt boundary, release outcome and successful
   cleanup. Only then remove the still-matching run marker. Retain the cleanup
   receipt and original lifecycle archive after removal.
8. Release the state-directory guard. Later ordinary startup may create a fresh
   session, but only a separately authorized assignment can spawn a worker.

The audit-write/marker-removal boundary must be idempotent: a crash after the
success receipt but before deletion repeats only guarded marker cleanup. A crash
after lease release but before success persistence repeats the owner-guarded
release safely. Failure to remove a marker is incomplete cleanup, not success.
Removing or replacing the ownership-lock file is never a recovery operation.

## Crash-Test Matrix

Use an isolated database, actual supervisor processes and two separately gated
worker fixtures. Kill only the fixture supervisor, not the whole test process
group, so a test cannot accidentally prove safety by killing workers itself.
Use readiness/release barriers rather than fixed startup sleeps. Independently
observe worker activity, server claim owners, lifecycle history, queue bytes,
positions, run markers and recovery audit outcomes.

| Fault boundary | Required result |
| --- | --- |
| Supervisor dies while both workers remain live | Recovery refuses both runs; no replacement worker starts and neither lease is taken from another owner. |
| Marker persisted but spawn outcome unknown | Refuse. Marker presence or absence of startup receipts must not imply that no process ran. |
| Worker stopped but terminal write failed | Refuse without sufficient stop provenance; do not synthesize terminal evidence from an empty pending queue. |
| Terminal receipt pending at supervisor death | With independently verified stop provenance, replay exact bytes, confirm owner release and clear only the matching marker. |
| Terminal accepted remotely but acknowledgement lost | Replay idempotently; the server retains one terminal event, not two. |
| Terminal acknowledged locally but marker remains | Use retained original evidence; do not requeue the archive or invent missing earlier receipts. |
| Lease release accepted but response lost | Repeat the same owner-guarded release; preserve any replacement owner's claim. |
| Cleanup success recorded but marker deletion interrupted | Repeated recovery removes only the matching marker and returns the recorded result. |
| Two recovery processes race | One holds the state-directory guard; the other performs no mutations. |
| One slot accounted for, one live or corrupt | Report per-slot results; never treat one successful cleanup as permission to reuse both workspaces. |
| Corrupt marker/frame, wrong identity, path escape or changed preview | Refuse before network mutation; compare bytes before/after to prove evidence was preserved. |
| Server position absent or outside the retained interval | Report inconsistent/unknown evidence and retain local state. |
| Permanent rejection or repeated disconnects | Preserve refused/pending frames; bounded retries do not extend the deadline. |
| Revoked identity or unavailable companion | No fallback credentials or local release; preserve recovery state. |
| Older outbox lacks already-pruned evidence | Report missing provenance; never reconstruct reports from sequence counters. |

Recovery is not qualified by a helper returning `Ok`, a stopped supervisor PID,
or a passing existing shutdown suite. The positive two-worker crash/restart test
must also show a later authorized fresh assignment executes exactly once and
cannot replay the recovered old assignment. Run applicable tests on Linux,
macOS and Windows; unavailable adapter guarantees are explicit refusals.

## Decisions Before Implementation

- The runtime owner must name the exact published commit/PR and stable connection,
  old-session receipt delivery, claim-release and lifecycle APIs. No shared-file
  edits proceed while the user-requested handoff wait remains unanswered.
- Agree the supported stop-provenance format. Existing marker and queue metadata
  cannot silently be expanded with guessed original registration, runtime or
  session start values. Unsupported older runs remain manual investigation.
- Choose and review the adapter mechanism for surviving-worker ownership if live
  crash recovery is in scope. It must cover descendants and crash/reboot/identity
  reuse, not just a parent PID. Do not implement that mechanism as a side effect
  of receipt replay.
- Agree confirmation identity, audit durability, input/scan limits, deadlines and
  filesystem revalidation semantics. SQLite read-only handles do not make a
  hostile same-user filesystem immutable; the threat boundary must be explicit.
- Establish a regression for each mutable boundary above before enabling any
  operator recovery command. Keep the command and each independently reviewable
  implementation step in its own scoped PR; do not accumulate unrelated work.

This proposal produces no process-stop proof, lease change, recovery capability,
or deployment claim. Its acceptance gate is design review; implementation and
platform qualification remain separate work.
