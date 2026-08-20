# ADR-0100: The repository node owns one non-exporting signer

- Status: Accepted
- Date: 2026-08-18
- Deciders: MindLeak maintainers
- Accepted: 2026-08-18 by the repository owner, authorized directly in
  session — attributed human adoption after review.
- Depends on: [ADR-0082](0082-ackplane-is-a-standalone-federation-service.md)
  (Ackplane is a standalone federation service),
  [ADR-0085](0085-node-enrolment-requires-proof-of-possession.md) (node
  enrolment requires proof of possession), and
  [ADR-0096](0096-ackplane-arbitrates-federated-claims-through-leased-delegation.md)
  (Ackplane arbitrates federated claims through leased delegation)
- Related: [ADR-0098](0098-connection-trust-reuses-the-enrolled-key-oidc-waits.md)
  (proposed connection authentication with the enrolled node key)

## Context

ADR-0085 requires a repository node to create its Ed25519 key locally, keep the
private key non-exportable where the operating system supports that property,
and otherwise use an explicitly configured workload secret provider. It also
requires restart recovery to restore the same key and binding, or to create a
new node identity. The server-side enrolment, activation, rotation, revocation,
envelope verification, and connection verification paths now have concrete
contracts for the public half of that identity.

The repository side does not. `ackplane-client` can call the delegated-claim
RPCs, and the authenticated `ClaimDelegationService` contract requires a
`ClaimAuthentication` signed by the enrolled node key. No repository-side
component owns that key, no client can request a signature, and neither local
plane has an approved way to load it. The only `SigningKey` construction in the
workspace is server test setup.

Putting a private seed in an environment variable would make the next call
work, but it would directly contradict ADR-0085's rejected static bearer secret
and its non-exportability requirement. Letting `mindleak-mcp` and
`lodestar-mcp` each load the key would create two key stores, two rotation
states, and two opportunities to continue after revocation. Moving the key to
Ackplane would turn the authority into escrow for every repository identity,
which ADR-0085 also rejects.

There is a second gap in the current request shape. `ClaimAuthentication`
carries `signed_at` and `nonce`, but verification does not bound the timestamp
or consume the nonce. Its signature binds tenant, repository, task, and owner,
but not the RPC operation or operation-specific fields. A valid authentication
can therefore be replayed indefinitely or reused across claim, renew, release,
and recover while changing lease, branch, scope, expected owner, or reason.
Identity proof is necessary, but it is not authorization of an exact mutation.

The product needs one repository-side owner for identity and outbound
federation, not another helper in each local plane.

## Decision

**A full-server repository runs one `ackplane-node` companion. It owns the
repository's enrolled identity, non-exporting signer, and outbound Ackplane
clients. Local planes never load, export, or persist the private key.**

1. **`ackplane-node` is the one repository-side identity owner.** It is an
   optional local companion process, one instance per MindLeak repository id.
   It owns the active tenant id, Ackplane repository id, node id, key id,
   enrolment receipt, provider handle, and outbound NodeSync and claim clients.
   A repository-scoped process lock refuses a second active instance. Ackplane
   remains the remote authority; the companion is the local holder of the
   credential that proves which enrolled node is calling it.

2. **The signer is capability-based and non-exporting.** The internal
   `NodeSigner` contract exposes only:

   ```text
   identity() -> { node_id, signing_key_id, public_key, fingerprint }
   sign(domain, binding, message_digest) -> signature
   provision_successor() -> successor identity handle
   retire(handle) / destroy(handle)
   ```

   There is no `private_key()`, seed export, serialization, debug rendering, or
   clone of private material. A signature request carries the expected tenant,
   repository, node, and key binding. The provider refuses a mismatch before
   signing. Secret-bearing types implement redacted diagnostics and zeroize any
   provider that must briefly expose bytes in process memory.

3. **Local planes use a narrow local capability, not the credential store.**
   `lodestar-mcp` sends claim, renew, release, and recover domain requests to
   the companion and receives the authoritative lease result it may project
   into its local read-through cache under ADR-0096. `mindleak-mcp` sends
   committed synchronization records and receives receipts. Neither plane
   calls an OS key API, links a cloud secret SDK, or receives a raw signature
   primitive that could sign arbitrary bytes. The companion validates and
   constructs the closed Ackplane request before signing it.

4. **Local IPC is an operating-system protected endpoint.** Windows uses a
   named pipe restricted to the installing user or service identity. Unix
   systems use a Unix-domain socket in the user-local MindLeak repository
   directory with owner-only permissions. The endpoint is repository-scoped
   and accepts only the closed node operations above, never arbitrary bytes to
   sign, MCP payloads, source patches, or terminal commands. TCP loopback and a
   reusable bearer token are not the default IPC because they replace one key
   custody problem with another copyable secret.

5. **Provider selection is explicit and fail-closed.** A provider URI in the
   user-local node metadata selects one of two classes:

   - An OS-backed provider uses a non-exportable signing key where the platform
     facility supports one: Windows CNG/key storage provider, macOS Keychain /
     Secure Enclave where available, or a configured PKCS#11/TPM provider on
     Linux and managed workstations.
   - A workload provider delegates signing to an explicitly configured HSM,
     KMS, vault, or workload secret facility. It is selected by provider and
     opaque key handle, not by placing a private seed in ordinary config.

   Unsupported provider schemes, missing handles, unavailable providers, and
   a public-key/fingerprint mismatch refuse startup in federated mode. They do
   not generate a replacement key and do not select local coordination.

6. **Raw private keys are not configuration.** Private material may not appear
   in repository files, command-line arguments, ordinary JSON/TOML settings,
   logs, telemetry, receipts, crash diagnostics, or plain environment
   variables. Environment variables may identify a provider endpoint, managed
   identity, or opaque key handle; they may not contain the Ed25519 seed. A
   development-only software provider stores its encrypted secret in the OS
   credential facility and is still excluded from repository state.

7. **Enrolment and restart recover the same provider identity.** On first
   enrolment the companion asks the selected provider to create the key, sends
   only its public key and fingerprint, and persists the provider scheme plus
   opaque handle beside the non-secret binding metadata ADR-0085 decision 6
   requires. On restart it resolves that handle, derives the public identity,
   and compares tenant/repository/node/key/fingerprint with the enrolment
   receipt before opening a stream or acquiring a claim. Missing or mismatched
   material means `identity_unavailable`; only a new attributed enrolment may
   create a replacement node.

8. **Rotation has two live handles and one authority transition.** The provider
   provisions a successor without exporting it. The companion signs the
   continuity request with both current and successor keys, keeps both handles
   for the bounded overlap Ackplane grants, and selects the key Ackplane says is
   active for each new operation. After Ackplane retires the prior key and the
   overlap drains, the companion asks the provider to retire or destroy the old
   handle. A suspected current-key compromise follows ADR-0085: revoke and
   freshly enrol; the compromised key cannot approve its successor.

9. **Revocation and provider loss stop new authority immediately.** A
   `node_revoked`, expired, retired, binding-mismatch, or provider-unavailable
   result closes live federation clients, clears no durable evidence, and
   refuses new signatures. Previously accepted records and receipts remain
   resolvable. Retry is permitted only for a retryable provider outage and is
   bounded; revocation and binding mismatch are terminal until an attributed
   administrative action changes the state.

10. **Claim authentication authorizes one exact operation once.** The canonical
    signed bytes include a distinct operation tag and every operation-specific
    field, in addition to tenant, repository, task, owner, node, and key. Each
    request uses a cryptographically random nonce and RFC3339 timestamp.
    Ackplane verifies a bounded clock window and atomically consumes
    `(signing_key_id, nonce)` before applying the claim CAS; a duplicate nonce,
    stale timestamp, changed field, or cross-operation replay is refused and
    audited. Client-selected nonces preserve the one-round-trip ownership call;
    if durable nonce consumption proves impractical, a later amendment may
    choose a server challenge and explicitly accept its extra round trip.

11. **Standalone mode has no companion cost.** `ackplane-node`, its credential
    providers, IPC client, Tokio, Tonic, and network stack are absent from the
    default standalone dependency closure. An undeclared repository remains
    locally arbitrated exactly as today. Selecting `federated` requires the
    companion, provider, enrolled binding, and reachable Ackplane; any missing
    prerequisite is a typed refusal, never silent local fallback.

12. **Tests use capabilities, not copied secrets.** Pure client and plane tests
    receive a deterministic fake `NodeSigner` that records the closed domain
    request and returns a test signature. Provider contract tests verify that
    raw key export is impossible through the public interface. Gated integration
    tests use an ephemeral software key registered in the test database and the
    real client/server canonicalization. Sabotage tests must prove that changing
    the RPC tag, lease, branch, scope, recovery fields, timestamp, or nonce
    invalidates or rejects the authentication.

## Consequences

- Authenticated claim routing is blocked until this ADR is accepted and its
  signer/provider boundary exists. That is intentional: a raw-key shortcut
  would become the de facto credential design.
- Full-server mode gains one additional local process and a platform-specific
  IPC/provider integration. In return, key custody, rotation state, outbound
  connections, and federation refusal live in one place instead of drifting
  between two MCP servers.
- The local planes remain deterministic and complete in standalone mode. In
  full-server mode they still own local memory, task content, evidence, and
  conformance; the companion owns only node identity and transport to the
  remote authority.
- Workload deployments can use managed HSM/KMS/PKCS#11 identities without
  copying a seed into a container. Developer machines can use their OS
  credential facility with the same `NodeSigner` contract.
- The claim-authentication wire contract needs an operation-specific canonical
  form and durable nonce consumption before production routing ships.
- Operational tooling must report provider kind, key id, lifecycle state, and
  last successful signature without ever reporting key material.

## Rejected alternatives

**Put the Ed25519 seed in an environment variable or config file.** Rejected
because it is exportable, copyable, commonly logged, and directly contradicts
ADR-0085's provider and non-exportability requirements.

**Let each MCP server load and use the node key.** Rejected because two local
planes would become two key stores with independent restart, rotation, and
revocation behavior. A shared credential entry does not make duplicated
lifecycle ownership coherent.

**Expose a generic `sign(bytes)` local RPC.** Rejected because any compromised
local caller could turn the node identity into an arbitrary signing oracle. The
companion accepts closed domain operations and constructs the canonical bytes
it signs.

**Run the signer on Ackplane.** Rejected because Ackplane would escrow every
node identity and could impersonate any producer. ADR-0085 deliberately keeps
private keys on the node side of the trust boundary.

**Use one static local bearer token to protect a loopback signer.** Rejected
because it creates another long-lived copyable credential and leaves process
identity and repository scope implicit. OS-protected local IPC provides the
boundary the platform already knows how to enforce.

**Generate a new key when the provider is missing.** Rejected because metadata
is not proof of identity. Silent replacement would fork the producer chain and
could resurrect a revoked node under its old name.

**Treat `signed_at` and `nonce` as descriptive fields only.** Rejected because a
field that changes no verification outcome is not a replay control. Freshness
must be checked and nonce use must be durable and single-use.
