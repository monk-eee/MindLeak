### Added

- Added the `ackplane-node` crate (ADR-0100 slice 1): the repository-side
  identity owner's `NodeSigner` capability contract (`identity`, `sign`,
  `provision_successor`, `retire`, `destroy`) and a development-only in-memory
  software provider. No `private_key()`/seed-export method exists on any
  public type, secret-bearing types redact their `Debug` output, and a
  repository-scoped process lock refuses a second concurrent instance.
  OS-backed providers (Windows CNG, macOS Keychain/Secure Enclave, Linux
  PKCS#11/TPM) and the local IPC endpoint are separate, narrow follow-on
  slices — this crate is not yet wired into `lodestar-mcp`/`mindleak-mcp` or
  usable in a real federated deployment on its own.
