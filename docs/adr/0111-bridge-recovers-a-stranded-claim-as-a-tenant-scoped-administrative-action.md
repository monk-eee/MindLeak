# ADR-0111: Bridge recovers a stranded claim as a tenant-scoped administrative action

- Status: Accepted
- Date: 2026-08-20
- Deciders: MindLeak maintainers
- Depends on: [ADR-0096](0096-ackplane-arbitrates-federated-claims-through-leased-delegation.md)
  (Ackplane arbitrates federated claims through leased delegation),
  [ADR-0098](0098-connection-trust-reuses-the-enrolled-key-oidc-waits.md)
  (connection trust reuses the enrolled key; OIDC waits),
  [ADR-0100](0100-repository-node-owns-one-non-exporting-signer.md) (the
  repository node owns one non-exporting signer)
- Refines: ADR-0105 decision 5 (the Bridge becomes the human control room
  for active work: release or recover leases among the named capabilities)
- Related: [ADR-0108](0108-knowledge-rpcs-authenticate-with-operation-signing.md)
  (Knowledge RPCs authenticate with an operation-signing scheme mirroring
  claims — the precedent this ADR explicitly does not follow, and says why)

## Context

ADR-0105 decision 5 names "release or recover leases" as part of the Bridge's
first Industrial workflow: coordinating agents from the Bridge. Bridge already
exposes a **read** of active claims (`GET /api/v1/repositories/:id/claims`,
`FleetStore::active_work`) — an operator can already see that a task is
claimed, by whom, and when its lease expires. There is currently no way to
*act* on that view. Every mutation — `delegate_claim`, `release_claim`,
`renew_claim`, `recover_claim` — is a `ClaimDelegationService` gRPC RPC, and
every one of those RPCs requires a `ClaimAuthentication`: an Ed25519 signature
from an enrolled node's non-exporting signing key, over operation-bound bytes,
consuming a single-use nonce (ADR-0100 decision 10). That is correct for the
RPC's *normal* caller — an agent or node contesting or renewing its own claim —
and this ADR does not touch it.

A human operator looking at the Bridge has no node signing key. They are not
the node that stranded the claim; they are the person who can see, from
outside any one agent's perspective, that a lease has genuinely expired and
nobody is coming back for it — exactly the situation this repository's own
practice repeatedly resolves by hand today (see the "Rescuing a lapsed claim
end-to-end" and "Adopting stranded work: a lapsed lease alone proves nothing"
patterns this project already follows manually, worktree by worktree, PR by
PR). Naively "fixing" this by having the human somehow acquire or borrow a
node's signing key would be worse than the current gap: it would hand a
human-operated Bridge session the exact cryptographic identity ADR-0100 built
specifically to be non-exportable and node-scoped.

This is not a variant of the gap ADR-0108 closed. ADR-0108 authenticates a
second RPC domain (`KnowledgeService`) the same way ADR-0100 already
authenticates claims, because both callers are the same kind of principal: an
enrolled node, proving it holds a key Ackplane already trusts. Bridge's
administrative caller is a different kind of principal entirely — not a node
at all — so mirroring ADR-0100's mechanism a second time would solve the wrong
problem: it would still require a signing key nobody legitimately using the
Bridge possesses.

ADR-0098 already decided how Bridge authorizes an administrative action in the
absence of a node key or an authenticated human principal (decision 4's OIDC
remains deferred): every Bridge query and administrative mutation carries an
explicit `tenant_id`/`repository_id` scope, checked by a lint/test-level guard
(`repository_id_guard.rs`), gated by the loopback-only developer profile
(`BridgeConfig::resolve` refuses any non-loopback bind without a production
verifier). That decision already anticipated administrative mutations, not
only reads — decision 5 explicitly names "policy activation, waiver grant,
evidence review, fleet operation, and audit read" as permissions that wait for
OIDC, but treats the *scope* (repository-id-bounded, loopback-gated) as already
settled. What ADR-0098 did not yet enumerate is which specific mutations are
safe to expose under that model today, before OIDC exists. Claim recovery is
the first concrete case, and it needs its own answer because not every claim
mutation is equally safe to expose this way.

The four claim mutations are not equally risky to place behind a
tenant-scoped-but-otherwise-unauthenticated administrative action:

- **`delegate`** is how a node *acquires* a claim it does not hold — an
  administrative caller has no legitimate reason to do this on a node's
  behalf, and doing so would let Bridge originate work attributed to a node
  that never asked for it.
- **`renew`** only extends a lease the *current* owner already holds, and
  requires knowing that owner's exact id — a human has no reason to do this
  either; a live node renews its own lease.
- **`release`** hands back a **live** lease before it naturally expires.
  `ClaimStore::release`'s own CAS is owner-guarded (`WHERE ... owner_id = $4
  AND lease_expires_at > $5`) but has no other safety net: if the caller
  simply names the right `owner_id`, a live claim is freed, interrupting work
  actively in progress. There is no cryptographic or temporal check standing
  between "administrative caller" and "took a live node's claim out from
  under it."
- **`recover`** is different in exactly the way that matters here.
  `ClaimStore::recover`'s CAS refuses unconditionally whenever
  `previous_expiry >= now` — a **live** lease is never recoverable, regardless
  of who calls it or what they claim to believe. The safety property that
  stops a live claim from being stolen lives in the store's own comparison
  against the database's clock, not in the caller's identity. Bypassing
  node-signing for `recover` therefore does not remove a real protection —
  the protection it would remove (proof that the caller is a legitimate
  node) narrows to a *different*, already-real protection (the lease is
  provably, unconditionally expired) that this ADR keeps in full force.
  `recover` also already requires a non-empty `reason`, which exists
  precisely to make a recovery decision auditable — a requirement this ADR
  reuses rather than invents.

## Decision

**Bridge exposes `recover` — and only `recover` — as a tenant-scoped
administrative action, calling `ClaimStore::recover` directly rather than
through the node-signed `ClaimDelegationService` RPC. `delegate`, `release`,
and `renew` remain unavailable through Bridge and are explicitly deferred, not
silently out of scope.**

1. **Bridge holds its own `ClaimStore` connection, exactly as it already holds
   `FleetStore` and `KnowledgeStore`.** No new service, no new protocol
   message: `AppState` gains a `claims: Arc<Mutex<ClaimStore>>` (a `Mutex` is
   required here, unlike `FleetStore`/`KnowledgeStore`, because `ClaimStore`'s
   mutating methods take `&mut self` — the same reason
   `ClaimDelegationService` itself wraps its store the same way). This is
   read-adjacent reuse of an existing type, not a new authority: the store's
   own CAS logic is the only thing that decides whether a recovery succeeds.
2. **The route is `POST /api/v1/repositories/:repository_id/tasks/:task_id/recover`,**
   accepting a JSON body of exactly `{ "owner_id": string, "reason": string,
   "branch": string, "lease_seconds": u64 }` — the same fields
   `ClaimRecoverRequest` already requires, minus `expected_owner`, which the
   handler derives itself: it reads the claim's *current* owner from
   `ClaimStore::list_active`/an equivalent lookup before calling `recover`,
   the same way a human today reads `owner` off `view=overlap` before
   deciding to rescue stranded work. This removes one chance for the
   operator to fat-finger the wrong expected owner and have the request
   silently no-op as a rejection.
3. **The enrolment-gate-first pattern every other Bridge route already
   follows applies here too:** `state.fleet.repository(&state.tenant_id,
   &repository_id)` is checked before touching `ClaimStore`, returning `404`
   for an unenrolled or cross-tenant repository exactly like
   `repository_timeline`/`repository_knowledge`. `repository_id_guard.rs`'s
   existing structural test already requires this handler to reference
   `state.tenant_id` like every other one; a third store-coverage test
   (mirroring the `FleetStore`/`KnowledgeStore` ones) is added for
   `ClaimStore`.
4. **`reason` is required and is not optional cosmetic text.** `ClaimStore::
   recover` already refuses an empty reason (`ClaimStoreError::MissingReason`);
   Bridge's handler does not relax that. The reason is what turns "the
   administrative caller can technically do this" into an auditable decision,
   matching how a human today writes a plain-English justification into a
   task's thread before rescuing stranded work.
5. **`delegate`, `release`, and `renew` are not exposed by this ADR.** This is
   a deliberate, narrower slice than ADR-0105 decision 5's full list, not an
   oversight:
   - `release`'s missing safety net (a live claim, freed on request) needs its
     own answer — most plausibly a confirmation step that names the
     interrupted owner and records a reason with the same rigor as recovery,
     or waiting for OIDC's authenticated principal (ADR-0098 decision 4) so
     "who force-released this and why" is a real identity, not "whoever could
     reach the loopback Bridge port." Either is a decision of its own,
     deliberately not made here.
   - `delegate` and `renew` have no legitimate administrative caller at all
     under the current model — a human does not acquire or extend a node's
     claim on that node's behalf — so there is no gap to close for them right
     now.
6. **This does not extend to Ackplane's other node-signed RPC surfaces.**
   Nothing here revisits ADR-0100 decision 10 for claims generally, or
   suggests the same bypass for any future authenticated domain (including
   ADR-0108's knowledge authentication once implemented). The justification
   is specific to `recover`'s own unconditional expiry check; it does not
   generalize to "administrative callers may bypass signing," which would be
   the wrong lesson to draw from this ADR.

## Consequences

- Bridge gains its first real claim **mutation**, not just a read — the first
  concrete piece of ADR-0105 decision 5's "Work control room" beyond viewing
  active claims. An operator who sees a stranded, expired lease in the
  existing claims view can now recover it into the open state (via a fresh
  claim from whichever node or agent takes it next) without a manual
  worktree-by-worktree, node-by-node investigation.
- `release`, `renew`, and `delegate` remain node-signed-only. Anyone reading
  this ADR alongside ADR-0105 decision 5's list should not assume the
  remaining three are "coming next automatically" — each needs its own
  reviewed decision, per point 5 above.
- `ClaimStore` gains a Bridge-side caller with no signing requirement. Any
  future change to `ClaimStore::recover`'s CAS logic (e.g., changing what
  counts as "expired") changes what Bridge can do too, without a
  corresponding change to Bridge's own code — worth remembering when that
  method is next touched.
- Implementation (the route, the `AppState` field, the owner-lookup helper,
  the guard-test extension, the UI control, and its tests) is separate,
  larger work gated on this ADR's acceptance — not included in this change.
- Implemented in the same change that accepted this ADR: `FleetStore` gained
  `claim_owner`, a single-claim lookup with no lease-expiry filter — needed
  because `active_work` deliberately excludes expired leases (`lease_expires_at
  > now`), which are exactly the claims `recover` exists to act on. The Fleet
  UI's existing active-work list therefore still cannot show a stranded claim
  by itself; an operator recovers one by task id (e.g., from Lodestar's own
  board), which the added standalone recovery form supports directly.

## Rejected alternatives

**Mirror ADR-0108: give Bridge its own signing key and authenticate as a
pseudo-node.** Rejected because it would hand a human-operated, loopback-only
session a *durable cryptographic node identity* — exactly the non-exportable,
node-scoped property ADR-0100 built specifically to keep off of anything that
is not the enrolled repository node itself. It would also not actually prove
anything true: the "node" Bridge would be authenticating as does not exist and
holds no real claim to release or recover on its own behalf.

**Expose all four mutations (delegate/release/renew/recover) now, since
ADR-0098 already allows tenant-scoped administrative mutations in general.**
Rejected because ADR-0098's existing decision establishes the *scope model*
(tenant/repository-id-bounded, loopback-gated), not a blanket judgment that
every possible mutation is equally safe under it. `recover`'s unconditional
expiry check is a real, mechanical safety property the other three do not
share; treating them identically would silently drop that distinction.

**Wait for OIDC (ADR-0098 decision 4) before exposing any claim mutation
through Bridge.** Rejected for `recover` specifically: its safety already does
not depend on caller identity, only on the store's own clock comparison, so
gating it behind an authenticated-principal system that does not exist yet
(and is explicitly deferred until a second real tenant exists to design
against) would withhold a mutation that is already safe to expose under the
scope model ADR-0098 already accepted. The same argument does not hold for
`release`, which this ADR leaves waiting.
