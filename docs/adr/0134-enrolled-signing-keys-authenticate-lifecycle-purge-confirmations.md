# ADR-0134: Enrolled signing keys authenticate Lifecycle purge confirmations

- Status: Accepted
- Date: 2026-08-27
- Deciders: MindLeak maintainers
- Accepted: 2026-08-27 by the repository owner through an explicit
  implementation choice in session.
- Refines: [ADR-0119](0119-industrial-administration-lifecycle-policy.md)
  decision 7 and [ADR-0128](0128-the-hardened-loopback-profile-is-the-verified-principal-for-self-hosted-administration.md)
- Depends on: [ADR-0085](0085-node-enrolment-requires-proof-of-possession.md),
  [ADR-0096](0096-ackplane-arbitrates-federated-claims-through-leased-delegation.md),
  and [ADR-0100](0100-repository-node-owns-one-non-exporting-signer.md)

## Context

ADR-0131 required a Lifecycle purge confirmation label to differ from the
preview requester. That label was attributed but caller-controlled: the same
Bridge caller could choose any different string and satisfy the check. It
recorded an audit label, not proof that a second credential approved a
destructive action.

The self-hosted Bridge profile has one salted loopback principal. It has no
second human identity provider, and ADR-0098 deliberately defers OIDC until a
real second organizational tenant exists. But Ackplane already has a separate,
durable, enrolled Ed25519 identity for each repository node. Its signing-key
registry binds a key to one tenant, repository, and node; it records lifecycle
state; claim authentication already verifies operation-bound signatures and
consumes nonces durably.

## Decision

**A Lifecycle purge preview and confirmation are authenticated by enrolled
signing-key credentials. Confirmation requires public-key material distinct
from the key material that created the preview.**

1. Preview and confirmation each carry a domain-separated signature that binds
   the exact tenant, repository, operation, and security-relevant request
   fields. A signature for a claim, envelope, preview, or one confirmation
   never authorizes another operation.
2. The Bridge resolves the claimed key against the enrolled signing-key registry
   at the current time. It refuses unknown, tenant/repository/node-mismatched,
   inactive, retired, revoked, malformed, stale, unsigned, or bad-signature
   credentials before executing a purge.
3. Each `(signing_key_id, nonce)` is durably consumed once through Ackplane's
   existing nonce ledger. Domain-separated signing bytes prevent a valid claim,
   preview, or confirmation signature from authorizing another operation; a
   replayed signed preview or confirmation is refused even if its request
   fields are unchanged.
4. The preview persists the verified requesting signing key, node, and public
   key fingerprint. A confirmation with the same public-key fingerprint is
   refused without writing a purge receipt, even if a rotation assigned that
   material a new key id, so a genuinely different verified credential can
   still confirm inside the existing window.
5. The receipt records the verified confirming signing key, node, and public
   key fingerprint, not a caller-supplied label. Existing unsigned previews
   are legacy records and cannot be confirmed; a new signed preview is
   required.
6. A browser does not gain a signing key merely by serving the Bridge page.
   It may submit a credential produced by an enrolled client, but unsigned
   browser requests fail closed.

## Consequences

- The purge flow now proves control of two distinct registered key materials:
  one for preview and one for confirmation.
- This does **not** prove that two different humans controlled those keys. A
  deployment requiring human identity or organizational separation still needs
  OIDC or another verified human identity system; this ADR makes no such claim.
- The API gains a signed confirmation contract and durable credential
  provenance. Clients must use the shared signing helper rather than invent
  their own serialization.
- The old `confirming_label` is no longer an authorization input or public
  receipt field. Historical rows retain their old data, but are not eligible
  for a new authenticated confirmation.

## Rejected alternatives

**Keep a distinct caller-provided label.** Rejected because string inequality
does not authenticate a second actor and can be forged by the requester.

**Treat the loopback tenant token as two principals by adding a second label.**
Rejected because one credential would still create both identities.

**Introduce a second local administrator secret.** Rejected by ADR-0128:
another local secret duplicates the existing key lifecycle without establishing
an independent trust boundary.

**Build OIDC now.** Rejected for this slice because ADR-0098 defers federation
until a real second organizational tenant supplies the identity-provider
requirements and threat model. Enrolled signing keys already provide a
testable, revocable, repository-scoped credential boundary today.
