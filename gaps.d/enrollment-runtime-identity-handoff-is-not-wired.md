- **Provider-backed CLI enrollment is not yet adopted by the supervisor and local planes - OPEN, STAB-02.**
  `crates/ackplane-server/src/bin/register-me/enrollment.rs::run_activate` now
  uses the persistent node provider, with request-before-RPC persistence,
  accepted key/receipt binding and native cross-process restart. It does not
  populate the legacy signer that
  `crates/ackplane-client/src/node_identity.rs::resolve_node_identity`
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
  The CLI now calls these capabilities and retains provider ownership through
  its NodeSync connection. The supervisor and MCP enrollment-status loader do
  not yet adopt provider state, and no long-lived companion owns retention or
  monitors loss on already-authenticated streams. CLI adoption was fixed this
  run; the remaining runtime handoff is not fixed.
  An operator can therefore finish enrollment yet still fail to start the
  supervisor with the same identity. Wire the persistent provider and
  non-secret binding handoff through the existing signer/IPC contracts; verify
  enrollment-to-runtime across process restart without exporting a seed or
  silently replacing an existing credential. Left open under
  `task:f60a0347a46d`; tested provider enrollment does not claim runtime wiring
  is done.
