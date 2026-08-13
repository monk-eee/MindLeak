# ADR-0085: Node enrolment requires proof of possession

- Status: Accepted
- Date: 2026-08-10
- Deciders: MindLeak maintainers
- Accepted: 2026-08-10 by the repository owner — attributed human adoption after
  review.
- Corrected: 2026-08-13 — decision 3's enumeration of a pending request omitted
  the public key that decision 2 requires and decision 5 cannot work without.
  The shipped wire contract carried exactly the nine items decision 3 listed, in
  order, so the omission propagated into the protocol. The enumeration now names
  the key. No decision changed; an incomplete list was completed.
- Depends on: [ADR-0082](0082-ackplane-is-a-standalone-federation-service.md)
  (federated coordination mode),
  [ADR-0084](0084-ackplane-evidence-has-explicit-trust.md) (remote identity and
  signed evidence)
- Related: [ADR-0016](0016-platform-packaging-and-registration.md) (local
  installation), [ADR-0038](0038-isolated-worktrees-shared-repository-state.md)
  (repository identity), [ADR-0045](0045-a-fleet-is-a-distributed-system.md)
  (one arbiter per shared resource)

## Context

ADR-0084 requires an enrolled node identity, a node-held signing key, and
short-lived workload credentials. It deliberately does not explain how an
unauthenticated fresh install acquires those things. That bootstrap is the most
sensitive point in the design: proof that a node possesses a key is useless if
an attacker can bind its own key to the repository first.

A copied API key or one-time bearer token makes whoever presents the token the
repository node. Generating the private key on Ackplane and downloading it
would make the service a key escrow and expose the key during bootstrap. Trust
on first use would let the first process that knows a repository URL claim its
identity, even though ADR-0038 explicitly says independent clones do not become
one repository merely because their remotes match.

The ceremony also needs unambiguous recovery. Containers and developer
machines are replaced; keys expire or are compromised; administrators revoke
access. Silently generating a replacement key would create a second producer
under an old name and break the evidence chain precisely when its identity is
being questioned.

## Decision

1. **Enrolment is an explicit state machine.** A node moves through
   `unenrolled -> pending -> approved -> activating -> active -> rotating|revoked`.
   Ackplane is the authority for that state and appends every transition with
   actor, time, tenant, repository, node, key fingerprint, and reason. A local
   configuration flag cannot activate or un-revoke a node.

2. **The node creates its signing key locally.** The initial envelope-signing
   key is Ed25519. The private key is non-exportable where the operating-system
   credential facility supports that property; otherwise it is stored through
   an explicitly configured workload secret provider. Only the public key and
   fingerprint enter an enrolment request. The mTLS certificate key is separate
   and may rotate on a shorter lifetime.

3. **A pending request names the exact identity being requested.** It contains
   a random request id, tenant id, explicit Ackplane repository id, proposed
   node id and display name, the public key and its fingerprint, requested
   capabilities, creation time, and expiry. A Git remote URL may help a person
   find the repository but cannot select or merge its identity automatically.

4. **An authenticated administrator approves the fingerprint, not a label.** A
   principal authorised for that tenant and repository approves or rejects the
   pending request through Ackplane. Approval records the exact fingerprint,
   capabilities, and expiry the administrator reviewed. Interactive clients
   display the same short fingerprint on the node and approval screen so the
   person can compare them. Automated provisioning must supply an already
   trusted workload identity or a public key pre-bound by that identity; it does
   not receive a universal bypass token.

5. **Activation requires fresh proof of possession.** After approval,
   Ackplane issues a single-use, short-lived nonce bound to the request id,
   tenant, repository, node, approved fingerprint, and server time. The node
   signs the domain-separated challenge. Ackplane verifies the signature,
   consumes the approval and nonce atomically, moves the binding to `activating`,
   and issues a short-lived client certificate plus an immutable enrolment
   receipt. Replaying either the approval or challenge returns the original
   completed result only when every bound field matches; otherwise it is
   rejected and audited.

6. **Activation is confirmed remotely and persisted locally.** The node stores
   the Ackplane endpoint, tenant id, repository id, node id, key id, enrolment
   receipt, and renewable credential metadata. It considers itself active only
   after an authenticated gRPC connection confirms the same binding and
   Ackplane appends the `active` transition. Until then it stays `activating`,
   may retry with bounded backoff, and cannot publish records or acquire claims.
   A timeout never selects local coordination mode. A revoked or mismatched
   binding ends retries with its non-retryable reason. The private signing key is
   never included in the receipt or ordinary config.

7. **Rotation proves continuity with both keys.** An active node generates the
   successor key locally, proves possession of it, and authorises rotation with
   its current key. Ackplane appends the new binding and permits a bounded
   overlap while in-flight records signed by the prior key settle. It then
   retires the old key. If the current key is suspected compromised, it cannot
   authorise its own trusted successor; an administrator revokes it and approves
   a fresh enrolment.

8. **Revocation is immediate for new authority and append-only for history.** An
   authorised administrator or incident workflow records a reason, revokes the
   key and workload credentials, terminates live streams, and refuses further
   records with a non-retryable `node_revoked` reason. Previously accepted
   envelopes and receipts remain resolvable with their key status at acceptance
   plus ADR-0084's later-compromise annotation.

9. **Losing the key loses the node identity.** A fresh container or machine may
   resume the same node only when an approved secret provider restores the same
   private key and binding. Otherwise it is a new node and follows a new
   approval. Ackplane never recovers, exports, or regenerates the lost private
   key, and matching repository metadata does not transfer the identity.

10. **Capabilities are least-privilege and reviewable.** Enrolment grants only
    the node operations needed for synchronization. Human policy approval,
    waiver grant, tenant administration, and auditor reads are not node
    capabilities. Expanding a node's capability set is another attributed
    approval, not a self-service update in a heartbeat frame.

11. **Expired and rejected requests leave no authority behind.** They remain in
    the administrative audit log but cannot be revived. The node creates a new
    request and key challenge; Ackplane does not extend an expired bootstrap
    because a client kept retrying.

## Consequences

- Stealing a public enrolment request or knowing a repository URL is
  insufficient to become that repository's node.
- Interactive setup has an intentional approval and fingerprint-comparison
  step. That cost buys an attributable binding at the boundary where local
  unauthenticated state becomes organisation evidence.
- Fully automated deployment remains possible through an existing workload
  identity or pre-bound public key, without introducing a long-lived bootstrap
  secret.
- Node replacement is explicit. Operational tooling must distinguish credential
  renewal, key rotation, node replacement, and compromise recovery rather than
  presenting all four as "reconnect".
- Ackplane needs a certificate issuer, key and certificate expiry monitoring,
  a revocation path, and an administrative queue for pending enrolments.
- The ceremony proves control of the enrolled private key. It does not prove
  that the machine is uncompromised or that evidence signed by it is true.

## Rejected alternatives

**Put a static Ackplane API key in an environment variable.** Rejected because
it is copyable, hard to scope to one node, commonly leaked into logs, and has no
proof-of-possession or safe rotation story.

**Use a one-time bearer token as the whole ceremony.** Rejected because the
first holder could bind an attacker's key. A short-lived challenge is useful
only after an authenticated administrator has approved the exact fingerprint.

**Generate and escrow node private keys on Ackplane.** Rejected because a
service compromise would expose every producer identity and because downloading
a private key creates an unnecessary interception path.

**Trust the first key seen for a repository URL.** Rejected because remote URLs
are not repository identities and trust on first use turns a race into
authority.

**Reuse the local `session_id`.** Rejected because logical session continuity is
not workload authentication, and a chat session is shorter-lived than a node
identity.

**Silently enrol a replacement after local state loss.** Rejected because it
would let local metadata impersonate a revoked or still-active node and would
hide a break in the producer chain.
