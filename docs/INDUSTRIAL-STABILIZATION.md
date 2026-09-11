# Industrial Stabilization

Started 2026-09-10. Goal: `goal:stabilize-the-industrial-product`.
Baseline: `b6f79e8da3ccb860f3541ea442d1aa791efbde32` on `origin/main`.
Implementation starts on `agents/industrial-stabilization` in an isolated worktree.

## Scope and Exit Criteria

Stabilize Ackplane, the Bridge, enrolled supervisors and their clients as one
operable product. Preserve the local planes, existing public contracts and
deterministic ingestion. This is integration and hardening, not a rewrite.

The pilot must demonstrate the following workflow from packaged artifacts:

```text
install -> enroll -> claim -> obtain context -> execute a real worker
        -> record evidence -> check conformance -> complete -> restart/recover
```

A healthy container, a process exit or a passing unit suite alone cannot satisfy
this gate. Two enrolled nodes must execute separate work without crossing task,
repository, tenant or worker boundaries. Accepted evidence must survive restart;
replays must not duplicate execution. Missing data and unsupported capabilities
must be visible failures, not successful empty results.

Remote Bridge exposure is prohibited until STAB-06 passes. The existing
loopback-only development topology is not a production authentication system.
No task below authorizes modifying a live database or exposing a service.

## Delivery Rules

- One active implementation task and isolated branch per workstream. Claim and
  renew before editing; record tests and failures against the owning task.
- Review and integrate existing worker/context/setup work through normal PR
  merges. Do not copy a peer's uncommitted files or rebuild over their checkout.
- Freeze new capabilities outside this roadmap. Narrow a task into tested changes
  when needed; do not add compatibility shims just to get a green build.
- Before closing a task, run its acceptance tests, required lint/build checks and
  conformance. Keep failed or unverified requirements open. Code completion and
  deployment qualification are different milestones.
- The ledger orders these tasks as a single-writer handoff chain. Security can be
  reprioritized with an explicit plan update; it must never be bypassed for a
  remotely accessible pilot.

## Task Board

Estimates are engineering days for one experienced engineer familiar with this
repository. They are provisional, not deadlines; reassess after STAB-03.

| Task | Ledger ID | Status | Estimate | Acceptance Summary |
| --- | --- | --- | --- | --- |
| STAB-01: Reproducible build and database gate | `task:52eb3f6d82bd` | Merged, CI passed, required check enabled | 1-3 days | Fail closed without database/recovery prerequisites; migrate explicitly; compile every target; run all-feature workspace tests against disposable Postgres in CI and locally. |
| STAB-02: Installation and enrolled identity | `task:f60a0347a46d` | In progress: companion runtime handoff tested; installation and review open | 3-5 days | Clean-machine TLS setup, tenant-consistent enrollment, persisted identity, actionable refusals and idempotent repeat setup. |
| STAB-03: Real worker execution and isolation | `task:df1e790eefca` | Queued after STAB-02 | 5-10 days | Addressed, authenticated work drives a real configured worker with bounded current context in its own worktree; two-node isolation and peer-impersonation refusals pass. |
| STAB-04: Completion and restart recovery | `task:a200ebd9ec16` | Queued after STAB-03 | 5-8 days | Attributed evidence and conformance govern completion; crashes, reconnects, duplicate messages, expired claims and lost outbox state cannot silently lose or repeat work. |
| STAB-05: Shared context and honest freshness | `task:8edce4b7d4a9` | Queued after STAB-04 | 4-7 days | Enrolled-node embedding production feeds shared recall; invalidation, cross-tenant refusal and explicit unembedded/stale/unavailable states are tested. Resolve the Work freshness design mismatch explicitly. |
| STAB-06: Production authentication | `task:9be81e654a23` | Queued after STAB-05 | 8-15 days | Real identity verification and per-operation tenant authorization cover reads, mutations and enrollment approval; browser request protections and negative security tests pass. |
| STAB-07: Operations and data recovery | `task:5c3861ce6973` | Queued after STAB-06 | 5-10 days | Upgrade and encrypted backup/restore drills reconcile data and evidence; database loss, pool saturation and write failures are bounded, visible and documented. |
| STAB-08: Artifact qualification and pilot | `task:f3efac2bfdeb` | Queued after STAB-07 | 3-5 days plus seven-day pilot | Versioned artifacts from one reviewed commit pass clean install, upgrade and real two-node workflows; a sustained pilot has no unresolved release blockers. |

This is roughly 7-13 engineering weeks plus pilot observation. The first useful
confined industrial demonstration targets STAB-03, not the completion of every
production requirement. A production deployment needs the entire chain and may
require more work after security and capacity measurement.

## First Gate

Requirements: Rust 1.88+, Node 20+, PostgreSQL 16 client tools (`pg_dump` and
`pg_restore`) on `PATH`, and a disposable pgvector/PostgreSQL 16 instance.
The role must be able to migrate schemas, install pgvector, and create/drop
scratch databases for recovery tests. Never use a production or shared live
deployment database.

Set both environment variables explicitly using the environment mechanism of
your terminal, task runner or CI system:

| Variable | Meaning |
| --- | --- |
| `ACKPLANE_TEST_DATABASE_URL` | Disposable database for ordinary integration fixtures. The runner migrates this database before executing tests. |
| `ACKPLANE_TEST_REHEARSAL_DATABASE_URL` | Maintenance connection on a disposable PostgreSQL instance, used to create and drop scratch databases for restore tests. It may be the same URL as the test database because the fixtures create separate databases. |

```bash
node scripts/industrial-test.mjs
```

`make industrial-test` is the same command. The runner refuses missing or invalid
URLs and missing client executables, overrides `ACKPLANE_DATABASE_URL` for its
children with the explicit test URL, then builds all workspace test targets,
runs the existing `migrate` binary, and executes the all-feature workspace suite
with two build jobs and four test threads. It stops at the first command failure
and preserves its exit status. It does not start, stop or reset any deployment.

The caller owns the disposable database lifecycle. URL validation is not proof
that a database is disposable; the runner cannot infer that from its name.
Existing ignored model tests retain their own prerequisites. Native credential
coverage is now mandatory in this gate, with an isolated Linux Secret Service
session; a passing database gate does not imply that optional models ran.

PR [#920](https://github.com/monk-eee/MindLeak/pull/920) merged as
`7991ba3a9aafe4d6a6012fd160b405856abb7b3c` after all applicable CI jobs passed.
With explicit user approval, **Industrial (database and recovery)** is now a
required check on `main`, preserving the six previous checks and strict
up-to-date enforcement. STAB-01 is complete with aligned conformance.

## Baseline Evidence

- Isolated `main`: all workspace targets compile with all features enabled.
- STAB-01 runner: six focused Node tests pass, covering missing configuration,
  credential-safe URL errors, ordering, live-URL replacement, command failure and
  terminated processes. The tests were introduced red before the runner existed.
- Full runner passes against fresh, migrated disposable PostgreSQL with both
  database gates enabled and PostgreSQL 16 recovery tools available. Repeating
  through `make industrial-test` passes 2,556 tests with zero failures and three
  existing ignored tests across 78 test targets. The disposable database was
  removed after validation.
- The active development checkout was changing during the initial audit; its
  transient compiler failures are not attributed to `main`.
- Remaining product gaps are tracked by STAB-02 through STAB-08. A green first
  gate does not mark those tasks complete.

The detailed acceptance text and ownership live in Lodestar. This document is a
portable roadmap, not a second task-state authority. Update evidence at each
reviewed checkpoint and retain the tested commit identity.

## Enrollment Identity Safety

This and the following recovery section describe earlier file-based CLI
checkpoints. Current commands and provider custody are described under
**Provider-Backed CLI** below; the old seed-file options are no longer accepted.

The first STAB-02 change preserves the existing ignored key file at
`.mindleak/ackplane-node.key` (or the explicit `--key-path` /
`MINDLEAK_ACKPLANE_KEY_PATH`). No second copy of the private key belongs in a
dotenv file. A dotenv may hold non-secret configuration, but `register-me` does
not load dotenv files itself; its caller must supply the environment.

An unreadable or malformed key is an error, never permission to regenerate it.
If enrollment state exists but its key is gone, restore the original key from
protected backup. Activation only loads an existing key and verifies its public
fingerprint against the saved enrollment before connecting. Only a new request
with neither a key nor saved enrollment may create a key; concurrent creation
publishes one complete key without overwriting a winner.

Validation: all 18 enrollment CLI tests pass, including the two accidental
replacement cases first reproduced against the old implementation. Scoped
Clippy is warning-free. The full industrial gate passes 2,562 tests with three
existing ignored tests against a fresh disposable PostgreSQL instance.

This does not complete STAB-02: clean installation, activation-to-runtime
configuration, idempotent enrollment and the administrative approval surface
remain to be verified and completed.

## Activation Recovery

`register-me activate` now writes the assigned `signing_key_id` and
`enrolment_receipt_id` under `activation` in the existing
`<key-path>.enrollment.json` file. The atomic write happens before NodeSync, and
contains no private seed. Malformed or mismatched activation responses cannot
replace the pending record. A failed write is a failure, not a reported success.

After a successful local save, repeating `activate` uses the recorded key ID
without requesting a second activation. The normal path still opens a fresh
authenticated NodeSync connection; a revoked or unavailable identity is not
made authoritative by a local file. The standalone `--skip-sync` flag reports
recorded activation only, including while the service is offline. New `request`
invocations refuse an existing enrollment record instead of overwriting it.

The CLI integration test uses real gRPC and disposable Postgres: activation
succeeds without a sync service, sync fails, the public identity survives a
new CLI process, and the same key authenticates twice after sync is restored.
Unit tests cover missing fields, wrong requests, rejected responses, file-write
failure and no-overwrite request persistence. Approval errors also retain their
failure category without repeating the password-bearing database URL.

The CLI also records the public `activation_nonce` before submitting proof. If
Ackplane commits activation but its response is lost, a new CLI process signs
that same nonce with the same approved key and uses the existing exact-replay
contract to recover the original receipt and key ID. A still-pending approval
may return a refreshed challenge; receipt persistence clears the retry nonce.
Failed writes preserve the previous retry state. No private seed or reusable
signature is saved in the sidecar.

The fault-injection test deliberately drops a successful server response and
then restarts the CLI. A corrupted nonce is refused, a valid retry returns the
original result, and the database still contains exactly one receipt and key.
The server's replay lookup now binds the key to that original receipt instead
of selecting the newest key for the same node. Existing proof verification and
bootstrap expiry checks remain in force; this is recovery of a recorded result,
not permission to reactivate an expired enrollment or claim current authority.

Losing the whole sidecar, including the retry nonce, still requires restoring
the protected enrollment state. Saving a receipt is not signer provisioning;
the current [Node Companion](#node-companion) completes the runtime handoff
without a copied seed or replacement identity.

## Persistent Node Provider

`ackplane-node::CredentialProvider` is an explicitly selected software provider
for the existing `NodeSigner` interface. Creation now belongs to
`CredentialCandidate::provision`, which has no signing-key ID until Ackplane
assigns one. `CredentialProvider::recover` restores only an activated binding;
`CredentialCandidate::recover` restores a pending candidate. Neither recovery
path creates a replacement key. Public metadata creation is internal to the
provider, not a second public enrollment API.

The credential includes its tenant, repository, node, challenge or activation
binding. Local `enrolment.json` contains only those public fields and a random
handle, never the private seed. Recovery checks both records, and signing
re-reads the credential so removal or replacement stops new signatures.
Provisioning refuses existing state; its initial metadata publication never
overwrites a filesystem entry, and failure removes only the newly created
credential. Activation updates retain the credential even when public metadata
publication fails, so recovery can finish the accepted binding. Errors and debug
output exclude secret bytes; transient secret buffers are zeroized.

Tests cover restart, no-overwrite provisioning, metadata rebinding, replaced or
missing credentials, write failure and redacted errors. An opt-in native test
provisions a random test credential, releases the provider, launches a separate
process, verifies a signature from the same restored public key, then deletes
only that test credential. macOS and Windows CI explicitly enable the test:

```bash
cargo test --locked -p ackplane-node a_persistent_provider_survives_a_real_process_restart -- --nocapture
```

Set `MINDLEAK_REQUIRE_CREDENTIAL_FACILITY=1` to require that native test rather
than skip it. It requires Keychain, Credential Manager or Linux Secret Service;
the ordinary isolated tests do not access a real credential store.

The initial persistence checkpoint passed all 29 node tests with native
credential access required, all-target/all-feature Clippy, and the full industrial gate: 2,588
passed, zero failed and three existing ignored tests across 79 targets.
Metadata-rebinding and no-overwrite regressions were checked in both failing and
fixed forms. The native restart test removed its randomly addressed credential.

This does not wire `register-me`, the supervisor or local planes to the provider,
does not adopt an existing file-based key, and does not claim hardware-backed
non-exportability. Persistent key rotation is refused, not approximated. STAB-02
stays open until the real enrollment-to-runtime path uses this ownership model.

## Crash-Safe Ownership

`NodeProcessLock` now uses a kernel-held exclusive file lock. A second process
is refused while the owner is live; the OS releases the lock when the owner
exits or is killed, even if Rust destructors never run. Restart does not delete
or regenerate the key, credential handle or enrollment record.

The `ackplane-node.lock` file intentionally remains after release. The PID in
it is diagnostic, not an ownership decision. Never remove the file to clear a
live lock: doing so can let two processes lock different files at the same
path. An unlocked marker is reused automatically, with no PID reuse heuristics
or stale-file timeout. These guarantees require a local filesystem and clients
using this lock. Stop older marker-only node processes before upgrading.

Process tests cover live-owner refusal, forced termination and restart,
independent repositories, and graceful release without unlinking the marker.
The native credential test also restores the same identity after a child exits
without destructors. This closes the node's stale-lock restart gap; it does not
claim worker-crash recovery, remote enrollment or a complete runtime handoff.

Local verification: 29 node unit tests and five process-ownership tests pass
with native credential access required. The full industrial gate passes 2,593
tests with zero failures and three existing ignored tests across 80 targets;
all-target/all-feature Clippy is clean. The stale-marker restart regression was
confirmed failing against the old lock before this fix.

## Provider Enrollment Binding

`CredentialCandidate::activation_proof` validates the approved challenge's
tenant, repository, node, fingerprint, request and nonce before persisting it
and signing the canonical enrollment bytes. `retry_activation_proof` reproduces
only that recorded proof. `accept_activation` checks the matching accepted
request and returned key ID and receipt, then turns the same credential into a
`CredentialProvider`; it never generates or exports a second key.

The credential stores the accepted binding before the public metadata is
replaced. Recovery completes an interrupted metadata update from that protected
record, but refuses a changed binding or an attempt to recover an activated
identity as a fresh candidate. Provider loss stops proof generation, activation
and runtime signing. An accepted receipt records a past decision; a new
authenticated connection still checks current authority.

The provider's old short fingerprint was incompatible with enrollment. A
regression test failed against it and passed after switching to the canonical
`ed25519:`-prefixed SHA-256 fingerprint. Fingerprint and activation encoders now
live in `ackplane-protocol::enrollment`; all server and test callers use that
implementation directly, with no compatibility re-export.

The real PostgreSQL/gRPC test discards an activation result, restores the same
candidate, rejects a changed nonce, and obtains the original key ID and receipt
by replay. It then recovers the provider and authenticates two fresh NodeSync
connections. The database still contains exactly one key and receipt. All 42
node unit tests and five process-ownership tests passed with database and native
credential checks enabled; all-target/all-feature workspace Clippy passed.
The full industrial gate passed 2,606 tests with zero failures and three
existing ignored tests across 80 targets, with both database gates and native
credential validation enabled.

This is a library capability, not a completed installation workflow. The existing
`register-me`, supervisor and local-plane clients still need to use it. The
client signing boundary and provider adapter are addressed below. Existing file-based
keys are not imported, old incomplete provider records are not silently
converted, and no seed-based or local-mode fallback has been added. STAB-02
remains open.

## Fallible Client Signing

`ClaimSigner::sign` now returns `Result<Vec<u8>, SigningError>`. Claim, purge
and recovery authentication propagate the same local failure without producing
a request. Federation handles the error before opening an RPC. NodeSync returns
`ClientError::Signing` and drops the unauthenticated stream before sending a
challenge response; a wire-level test verifies that only `Hello` was sent.
The existing callers were migrated directly, with no infallible compatibility
path, panic-on-refusal adapter or empty-signature fallback.
The capability also retains `Send + Sync`: a compile-time regression failed
with the original unbounded trait and passed after the fix, proving that its
connection future can be sent to a runtime worker.

`CredentialProvider::open_connection` uses the reusable NodeSync client through
a private adapter, with tenant/repository/node/key values from its recorded
binding. The adapter rechecks the credential for every signature and returns
only fixed, non-secret error categories. The real PostgreSQL/gRPC test now uses
this operation for both recovered connections, then removes the credential
while the provider is live and checks for a local signing failure.

All 25 client and 44 node unit tests pass with PostgreSQL and native credential
validation required. The final industrial gate passed 2,610 tests with zero
failures and three existing ignored tests across 80 targets, with both database
gates and native credential validation enabled. Workspace all-target/all-feature
Clippy passed with warnings denied. The initial contract probe could not compile
against the infallible API; the completed refusal tests cover all three authentication
families and the handshake's no-response behavior. A new test initially assumed
requested capabilities were echoed by `HelloAccepted`; it was corrected to the
server's existing explicit-enablement contract without changing server behavior.
One full-run attempt stopped on the unchanged worker-adapter test's
fixed-delay completion assertion (`Started` instead of `Completed`); the test
passed alone and in the unchanged full-suite rerun. The runtime workstream
subsequently replaced the fixed delay with TCP readiness and bounded exit
observation in PR #922. That exact fix is now included in this integration,
so its gap fragment is closed because the fix landed, not because a rerun passed.

This removes the client-signing blocker, not the remaining installer or companion
work. The caller must retain the provider's ownership while using its connection.
CLI adoption is described below; the supervisor and local planes still need this lifecycle;
legacy seed/cached-signing paths are unchanged. This does not monitor provider
loss or revoke authority on streams that were already authenticated. STAB-02
remains open.

## Provider-Backed CLI

`register-me request` now requires `--provider credential-facility-software` and
an absolute `--state-dir`. Missing or unsupported providers are refused before
any identity write or network request. The raw seed-file creation and loading
module has been removed; legacy key options are rejected without modifying their
files. The provider remains software key custody, not hardware non-exportability.

The CLI saves an immutable public request before contacting Ackplane. Repeating
an identical request preserves its ID, key, original timestamps and seven-day
expiry; changed parameters fail without replacement. Provider challenge and
activation records are not copied into the CLI descriptor. `activate` uses the
provider's exact-proof replay and returned authority binding, authenticates
NodeSync through the shared client, and publishes one deterministic enrollment
event that can be replayed after a lost receipt. `--skip-sync` remains a report
of historical activation, not current authorization.

Validation includes the provider-selection regression, confirmed failing when
the old command created a seed before refusal and passing after the migration.
The existing real CLI failed-sync and lost-activation-reply tests now use native
credential storage. New subprocess tests cover a lost initial request reply,
same-key restart with an unreachable server, changed-request refusal and
credential removal. An initial test-cleanup implementation blocked because
macOS `keyring::delete_password` reads a credential before deleting it, requiring
authorization across executables; reference-only lookup/deletion now removes
only the exact test-owned entry and retains metadata if cleanup fails.

The industrial runner now requires native credentials. `credential-test.mjs`
creates a fresh D-Bus session and private temporary Secret Service directories on
Linux; macOS and Windows use their native stores. Twelve runner tests pass,
and a real isolated Linux Secret Service round trip verified the wrapper.
Industrial and coverage CI use it; Windows/macOS CI explicitly run the native
CLI restart test. No existing database-gated CLI test is disabled.

Local verification passed all 26 CLI unit tests and four real subprocess tests.
The full locked industrial gate passed 2,610 tests with zero failures and three
existing ignored tests across 80 targets; database, recovery and native
credential checks were enabled. Workspace all-target/all-feature Clippy passed
with warnings denied. The two entries left by early cleanup failures were
removed by their exact test handles without reading passwords; successful tests
now verify cleanup, and the disposable test database was removed.

See [Provider-backed enrollment](../DEVELOPERS.md#provider-backed-enrollment)
for current commands. The following companion checkpoint supersedes this
historical CLI-only runtime boundary. Production administrator authentication
and clean installation qualification remain open.

## Node Companion

`register-me serve --state-dir ABSOLUTE_STATE_DIR` now retains the activated
provider as a long-lived identity and connection owner. The supervisor, MCP
front door and federated Lodestar resolve only the same directory, tenant and
repository. They do not load private keys. The existing demo constitution
publisher uses closed companion operations too. Legacy runtime node/key/seed
overrides and the old IPC signing/destruction surface are removed or refused.

Protected Unix sockets and Windows named pipes carry bounded, versioned,
repository-scoped requests. Supervisor streams additionally declare the exact
supervisor/session/worker binding, preserving replay-before-session ordering.
No Hello, challenge response, event batch or unrelated frame can be tunneled
through this local surface. Authority refusal retains its non-retryable category;
frame rejection remains a frame rejection so durable outboxes cannot skip it.
Thirty-two streams leave capacity for short-lived status and lease operations.

The node checks its provider and performs a fresh authority handshake every
second, with a five-second remote deadline. A failed check closes existing
streams and exits unsuccessfully. A supervisor that loses its local identity
owner stops workers and retains unconfirmed cleanup evidence instead of running
forever in a reconnect loop. Remote stream disconnects remain retryable while
the companion is healthy. Recovery uses the same provider, receipt and key;
an old worker marker is still not proof that its processes stopped.

Validation includes real native enrollment and companion subprocess restart,
verified status and supervisor frames, duplicate-owner refusal, full stream
capacity, changed-endpoint refusal, credential removal and server key revocation.
Fifteen real-worker scenarios exercise independent prompts, claims, context,
durable outcomes, shutdown/replay and owner loss through IPC. The actual MCP
process opens sessions and checks status beside a concurrent companion stream;
federated claims round-trip through the real service and PostgreSQL. Generic
signing, seed acceptance, cross-scope receipt forwarding, permission-refusal
mapping and endless reconnect after companion loss have focused red/green
regressions. The Windows ACL module and its test type-check for the Windows
target; native Windows/Linux qualification is a CI gate, not implied by macOS.

The final local industrial gate passed 2,689 tests with zero failures and three
existing ignored tests across 85 targets. Both database gates and native
credentials were required. Workspace all-target/all-feature Clippy passed with
warnings denied; relative links, gap fragments and changelog fragments validated.

STAB-02 is not complete. Clean-machine installation qualification and human
conformance review of the provider stack remain open, including the recorded
claim-lapse history and absent goal bindings. This checkpoint does not waive
those findings, approve pending PRs, claim hardware non-exportability, or complete
worker recovery and the later production-authentication/pilot milestones.

## Industrial Host Installation

`make install-industrial` now builds the six host binaries with a locked release
build and both local planes' federation features, then installs them through the
existing shared installer. Direct Cargo/Node equivalents are documented in
[the development guide](../DEVELOPERS.md#install-industrial-host-binaries).
The default Local installation remains independent and installs only its two MCP
servers. Industrial installation refuses missing/non-file/unreadable sources
before changing an executable, requires release builds without debug fallback,
and honors `CARGO_TARGET_DIR` instead of selecting stale workspace output.
Per-binary replacement stages the new file first so a copy failure preserves
the current command; repeat installation is supported. Installation does not
modify provider state or start/stop a service, and multiple binaries are not
published as one atomic filesystem transaction.

This removes the missing stable host-binary install path. Clean-machine TLS,
independent administrator approval, packaged distribution and the existing
conformance review remain separate requirements. The demo-setup workstream owns
the running-stack orchestration; no live deployment is changed by this installer.
