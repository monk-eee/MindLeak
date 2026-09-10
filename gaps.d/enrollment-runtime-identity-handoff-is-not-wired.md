- **Enrollment activation does not provision a restart-capable runtime identity - OPEN, STAB-02.**
  `crates/ackplane-server/src/bin/register-me/commands.rs::run_activate` now
  preserves the assigned public key ID and receipt, but it does not provision
  the signer that `crates/ackplane-client/src/node_identity.rs::resolve_node_identity`
  expects. That resolver still needs explicit identity environment variables
  and an already populated OS credential entry (or the explicitly non-hardened
  seed override). The accepted owner in ADR-0100 is `ackplane-node`; its exported
  `SoftwareProvider` is explicitly memory-only, not a persistent restart path.
  An operator can therefore finish enrollment yet still fail to start the
  supervisor with the same identity. Wire the accepted persistent provider and
  non-secret binding handoff through the existing signer/IPC contracts; verify
  enrollment-to-runtime across process restart without exporting a seed or
  silently replacing an existing credential. Left open under
  `task:f60a0347a46d`; the receipt-persistence change does not claim this is done.
