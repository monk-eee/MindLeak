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
- **The activation response can still be lost before its local save.**
  A process crash, transport loss or write failure after the server commits
  activation but before `SavedRequest::record_activation` persists the response
  can leave only the pending local record. `GetActivationChallenge` is not a
  receipt-recovery operation. Add a possession-authenticated recovery of the
  existing receipt and key binding, not a second activation or replacement key.
  Exercise that exact failure window against the real service; left open under
  the same STAB-02 recovery work.
