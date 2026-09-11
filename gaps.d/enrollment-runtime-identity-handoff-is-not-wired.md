- **Enrollment activation does not provision a restart-capable runtime identity - OPEN, STAB-02.**
  `crates/ackplane-server/src/bin/register-me/commands.rs::run_activate` now
  preserves the assigned public key ID and receipt, but it does not provision
  the signer that `crates/ackplane-client/src/node_identity.rs::resolve_node_identity`
  expects. That resolver still needs explicit identity environment variables
  and an already populated OS credential entry (or the explicitly non-hardened
  seed override). The accepted owner in ADR-0100 is `ackplane-node`. It now has
  explicitly selected `CredentialCandidate` and `CredentialProvider` types
  alongside the memory-only `SoftwareProvider`, with tested OS-credential-backed
  restart, binding checks and no key-export API. Candidate proofs, accepted
  authority key-ID/receipt binding, interrupted-write recovery and exact replay
  are now implemented. A real PostgreSQL/gRPC test replays activation and
  authenticates two recovered NodeSync connections with one key and receipt.
  The incompatible short provider fingerprint was fixed this run with a
  failing-then-passing regression and one shared canonical protocol encoder.
  The client's `ClaimSigner::sign` and authentication helpers now propagate
  typed provider failures; the NodeSync handshake sends no response after a
  local refusal. `CredentialProvider::open_connection` now uses the reusable
  client with its own binding and a private signer adapter, tested for recovered
  authentication and credential loss. The erased signer retains `Send + Sync`,
  verified by a failing-then-passing runtime-worker future regression. This
  signing boundary was fixed this run.
  The CLI and supervisor do not yet call these capabilities, and no companion
  yet owns provider retention or monitors loss on already-authenticated streams.
  An operator can therefore finish enrollment yet still fail to start the
  supervisor with the same identity. Wire the persistent provider and
  non-secret binding handoff through the existing signer/IPC contracts; verify
  enrollment-to-runtime across process restart without exporting a seed or
  silently replacing an existing credential. Left open under
  `task:f60a0347a46d`; tested provider enrollment does not claim runtime wiring
  is done.
