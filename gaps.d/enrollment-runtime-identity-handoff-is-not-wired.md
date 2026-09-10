- **Enrollment activation does not provision a restart-capable runtime identity - OPEN, STAB-02.**
  `crates/ackplane-server/src/bin/register-me/commands.rs::run_activate` now
  preserves the assigned public key ID and receipt, but it does not provision
  the signer that `crates/ackplane-client/src/node_identity.rs::resolve_node_identity`
  expects. That resolver still needs explicit identity environment variables
  and an already populated OS credential entry (or the explicitly non-hardened
  seed override). The accepted owner in ADR-0100 is `ackplane-node`. It now has
  an explicitly selected `CredentialProvider` alongside the memory-only
  `SoftwareProvider`, with tested OS-credential-backed restart, binding checks
  and no key-export API. That local provider is not yet connected to remote
  enrollment, assigned key IDs or the supervisor's runtime client.
  An operator can therefore finish enrollment yet still fail to start the
  supervisor with the same identity. Wire the persistent provider and
  non-secret binding handoff through the existing signer/IPC contracts; verify
  enrollment-to-runtime across process restart without exporting a seed or
  silently replacing an existing credential. Left open under
  `task:f60a0347a46d`; provider persistence alone does not claim this is done.
- **The node process lock does not recover automatically after a crash - OPEN.**
  `crates/ackplane-node/src/process_lock.rs::NodeProcessLock` uses exclusive
  creation and removes its file on `Drop`; a killed process can leave that file
  behind. The persistent provider reuses this lock and safely refuses a second
  owner, but a crashed node needs operator intervention before restart. Add
  crash-safe ownership or verified stale-lock recovery without allowing two live
  owners; test process termination and restart on the supported platforms. Left
  open under STAB-02, not silently treated as a passing restart scenario.
