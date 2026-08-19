- **No client anywhere in this repository has ever enrolled a repository or
  published a record to Ackplane, so the shipped Bridge Fleet view can never
  show real data — OBSERVED 2026-08-19, left OPEN.** ADR-0100 already names
  the missing piece abstractly ("a full-server repository runs one
  `ackplane-node` companion... [it] owns the repository's enrolled identity");
  this fragment records the concrete, currently-blocking consequence: not one
  line of client-side code anywhere in the workspace calls
  `SubmitEnrollmentRequest`, `GetActivationChallenge`, `ActivateEnrollment`, or
  `Synchronize` (`crates/ackplane-protocol/proto/mindleak/ackplane/v1/node_sync.proto`).

  `git grep`-confirmed across every crate: the only appearances of those RPC
  names in the entire repository are the `.proto` definition itself and
  `ackplane-server`'s own service-handler implementation/tests. The one
  client crate that exists, `ackplane-client`, implements only the four
  `ClaimDelegationService` RPCs (`DelegateClaim`/`ReleaseClaim`/`RenewClaim`/
  `RecoverClaim`) via `ClaimClient` — nothing for enrollment or data
  synchronization. Its own single test that touches enrollment
  (`tests/arbitration.rs`) does so by importing `ackplane_server::
  enrollment_store::EnrollmentStore` directly and writing an already-approved
  row straight into the test database, bypassing gRPC and the real
  submit-approve-activate lifecycle entirely — a fixture shortcut, not
  evidence the client-facing flow works.

  Impact: the Bridge (`ackplane-bridge`, ADR-0094/0095) Fleet screen
  (`docs/adr/0095-the-bridge-uses-an-authenticated-projection-api.md`,
  shipped and merged) reads real, already-built UI against a live
  `ackplane-server` + Postgres deployment and correctly renders "0
  repositories visible / Nothing enrolled yet" -- because nothing has ever
  enrolled one, and there is currently no way to, short of hand-writing a raw
  tonic gRPC client against the generated stubs from scratch. A person trying
  to see the shipped Fleet feature work end-to-end with real data hits this
  wall immediately, with no CLI, example, or documented recipe pointing the
  way -- the closest thing, `ackplane-server/tests`, only ever drives the
  service from the server side.

  Left OPEN: no fix attempted this run. The right-sized fix is almost
  certainly not the full `ackplane-node` companion (ADR-0100 decisions
  1/3/4/7-9, deliberately deferred, larger work) but something much smaller:
  a minimal client (even a throwaway example under `crates/ackplane-client/
  examples/` or a script) that submits an enrollment request, waits for
  approval, completes activation with a real Ed25519 proof, and opens one
  `Synchronize` stream -- enough to let a demo or a developer see one real
  repository actually appear in the Fleet view, without committing to the
  full companion-process architecture.
