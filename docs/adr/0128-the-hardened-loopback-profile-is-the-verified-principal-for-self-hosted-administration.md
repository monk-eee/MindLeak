# ADR-0128: The hardened loopback profile is the verified principal for self-hosted Industrial administration

- Status: Accepted
- Date: 2026-08-26
- Deciders: MindLeak maintainers
- Accepted: 2026-08-26 by the repository owner, authorized directly in
  session — attributed human adoption after review.
- Refines: [ADR-0119](0119-industrial-administration-lifecycle-policy.md)
  decision 2 (privileged administration classes require an authorization
  basis stronger than the loopback developer profile)
- Depends on: [ADR-0098](0098-connection-trust-reuses-the-enrolled-key-oidc-waits.md)
  decision 3 (the salt-derived, per-installation development tenant token),
  [ADR-0088](0088-the-ackplane-runs-in-containers-the-planes-do-not.md)
  decision 7 (Compose backup, restore, and reset), [ADR-0086](0086-postgresql-is-the-ackplane-ledger-and-arbiter.md)
  decision 12 (durability claims match deployment reality)
- Related: [ADR-0013](0013-local-data-lifecycle.md) (Local's own lifecycle
  model, the parity source), [ADR-0105](0105-bridge-is-the-server-version-of-the-vsix.md)
  decision 6 (Backup / export / reset parity row)

## Context

ADR-0119 decision 2 refused every privileged Administration class —
snapshot, export, recovery execution, lifecycle purge — until "a verified
principal with an adopted policy... stronger than the current loopback
developer profile" exists, naming the loopback profile itself as exactly
the thing that basis must exceed. ADR-0098 decision 4 defers OIDC until
Ackplane serves a second, independent organisational tenant with its own
identity provider. Read together, those two decisions leave every
privileged Administration class waiting on a prerequisite that ADR-0098
itself says must not be built speculatively — a real deadlock for the
single-operator, self-hosted deployment ADR-0098 says is the actual
near-term shape of this product.

That deadlock rests on treating "verified principal" as a synonym for
"OIDC-authenticated human." It is not. ADR-0098 decision 3 already hardens
the loopback developer profile specifically so it cannot be impersonated by
a guessed or logged tenant name: `development_tenant_token =
hex(SHA-256(salt || tenant_name))`, with a salt generated once per
installation and held beside other local secrets. That token is not "no
principal" — it is proof of possession of a file on the machine the
operator deployed Ackplane onto, which is exactly the trust boundary a
self-hosted, single-tenant Docker Compose deployment (ADR-0088) has: one
operator, one machine (or one operator's private network), one trust
domain, already gating claim recovery (ADR-0111) as the profile's first
mutation. Requiring a second, heavier identity ceremony before that same
operator can request a snapshot of their own database does not add safety;
it adds a wait for infrastructure ADR-0098 explicitly decided not to build
yet.

Multi-tenant Ackplane is a different trust boundary and this ADR does not
touch it: a deployment serving several independent organisations cannot
treat "possession of the operator's salt file" as any one tenant's verified
principal, because there the salt file is the platform operator's secret,
not each tenant's. ADR-0098 decision 4's OIDC requirement stands unchanged
for that case.

## Decision

**For a self-hosted, single-tenant Ackplane deployment (ADR-0088's Compose
topology or an equivalent single-operator install), the hardened loopback
developer profile (ADR-0098 decision 3's salt-derived token) is the
verified principal ADR-0119 decision 2 requires for privileged
Administration classes. Multi-tenant Ackplane still waits for ADR-0098
decision 4's OIDC before enabling them.**

1. **The salt-derived tenant token is the principal identity administration
   receipts record**, not a placeholder for one. `AdministrationApiState`'s
   `tenant_id` — already the same salted token every other Bridge route
   authorizes against — is recorded as the requesting principal on every
   administration request and receipt (ADR-0119 decision 3). It is not a
   stand-in "system" actor; it is the actual accountable identity for a
   deployment with exactly one operator.
2. **The adopted-policy requirement stays.** ADR-0119 decision 2 also
   requires "an adopted policy or delegation that names the operation,
   tenant and repository reach, data classification, retention basis, and
   effective lifetime" — this ADR removes only the *principal* half of that
   prerequisite, not the *policy* half. A deployment still cannot request a
   snapshot, export, or purge without an explicit, stored authorization
   record naming that operation, scope, and lifetime; the loopback profile
   establishes *who* is asking, not that *anything* they ask for is
   pre-authorized.
3. **A production multi-operator or multi-tenant deployment must not
   silently inherit this.** `BridgeConfig::resolve`'s existing refusal of
   any non-loopback bind without a production verifier (ADR-0094) is the
   enforcement point: the moment a deployment stops being the
   single-operator loopback shape this ADR is scoped to, it already refuses
   to start without stronger authentication, so it cannot reach a
   privileged Administration route under this looser basis by accident.
4. **This does not reopen any other ADR-0119 refusal.** Bounded/redacted
   export contracts, snapshot verification and encryption, recovery's
   inspection/rehearsal/execution separation, and lifecycle purge's
   refusal-first and audit-preserving design (ADR-0119 decisions 4-7) are
   unchanged; they gain an available principal, not a loosened contract.

## Consequences

- Snapshot, export, recovery inspection, and lifecycle purge can now be
  implemented and receipted against the existing loopback developer profile
  for the deployment shape this repository actually runs today, rather than
  waiting on OIDC.
- A future multi-tenant Ackplane still needs ADR-0098 decision 4's OIDC (or
  an equivalent verified human identity) before enabling these same routes
  for that deployment shape; this ADR grants multi-tenant no new authority.
- Every administration receipt now carries a principal identity worth
  recording, closing the "no subject to grant a permission to" gap ADR-0098
  decision 5 named for administrative actions generally.

## Rejected alternatives

**Wait for OIDC before implementing any privileged Administration class.**
Rejected because ADR-0098 already decided OIDC should not be built
speculatively before a second real tenant exists, which means this
alternative blocks the entire Administration parity row (ADR-0105 decision
6) on a prerequisite this repository has already decided not to build yet —
an indefinite wait with no real safety gained for the single-tenant
deployment that exists today.

**Invent a new, separate administrator credential (a second local secret, an
admin API key) distinct from the salt-derived tenant token.** Rejected
because it duplicates a hardening ADR-0098 decision 3 already built for
exactly this purpose, and a second local secret store is a second thing to
rotate, lose, or leak without adding a distinct trust boundary — the salt
file and the administrator are already the same person on this deployment
shape.

**Let a multi-tenant deployment share this ADR's basis too.** Rejected
because the salt-derived token authenticates possession of the *platform
operator's* installation secret, not any individual tenant's identity;
treating it as a tenant principal in a multi-tenant deployment would let
one operator's secret authorize privileged actions across tenants that
never agreed to trust that operator.
