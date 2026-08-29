# ADR-0142: The hardened loopback profile is also the verified principal for Work commands, Design mutations, and Constitution proposals

- Status: Accepted
- Date: 2026-08-29
- Deciders: MindLeak maintainers
- Accepted: 2026-08-29 by the repository owner, authorized directly in
  session — attributed human adoption after review.
- Refines: [ADR-0125](0125-bridge-work-commands-are-principal-scoped-and-receipted.md)
  decision 2 (which principal a Work command needs), [ADR-0123](0123-bridge-exposes-a-first-industrial-design-mutation-slice.md)
  (which relied on store-level safety instead of a caller identity because
  none existed yet), [ADR-0126](0126-the-bridge-proposes-constitution-amendments-only-a.md)
  (whose own text calls its author attribution "honest but weak until
  ADR-0098/OIDC lands")
- Depends on: [ADR-0128](0128-the-hardened-loopback-profile-is-the-verified-principal-for-self-hosted-administration.md)
  (the precedent this ADR extends: the salt-derived tenant token is a real
  verified principal for a self-hosted, single-tenant deployment, not a
  synonym for "no principal"), [ADR-0098](0098-connection-trust-reuses-the-enrolled-key-oidc-waits.md)
  decision 3 (the hardened token itself) and decision 4 (multi-tenant still
  waits for OIDC, unchanged by this ADR), [ADR-0094](0094-the-bridge-preserves-standalone-operation.md)
  (a non-loopback bind already refuses without a production verifier)
- Related: [ADR-0107](0107-registered-agents-accept-authenticated-control-directives.md)
  (the directive/receipt chain that already authorizes a supervisor-directed
  Work command's downstream effect independently of this ADR),
  [ADR-0111](0111-bridge-recovers-a-stranded-claim-as-a-tenant-scoped-administrative-action.md)
  (the original store-safety-not-identity precedent ADR-0123 borrowed, and
  which ADR-0125's own text says "is not a general precedent for that
  shortcut" -- this ADR is what actually settles that tension)

## Context

Four ADRs about Bridge mutation authority were accepted within three days of
each other, in an order that left them contradicting one another:

- **2026-08-24, ADR-0123**: Bridge's first Design mutations (propose,
  record a decision, record a materialization) ship with **no caller
  identity at all**. Each accepts a free-text `actor`/`proposed_by` field the
  HTTP caller supplies directly, safe only because `DesignStore` itself
  refuses an unsafe concurrent write (idempotent creation, a compare-and-swap
  on `lifecycle_state`) regardless of who calls it. ADR-0123 is explicit that
  this borrows ADR-0111's "safety lives in the store, not the caller"
  reasoning, and explicit that "legality-of-transition policy, per-operator
  identity, separation of duties... remain deferred."
- **2026-08-25, ADR-0125**: Bridge's Work commands take the opposite
  position -- decision 2 requires "a verified principal resolved by the
  ADR-0095 authentication verifier" for *every* command, and states plainly
  that "the current loopback development tenant token does not satisfy this
  requirement," so every one of the ten commands (`CreateWork`, `RouteWork`,
  `ReleaseLease`, `AnswerWait`, `SubmitReview`, `Assign`, `Steer`, `Pause`,
  `Resume`, `Drain`) resolves to a permanent, typed
  `authorization_unavailable` refusal under that profile. This shipped
  exactly as designed (PR #821): the routes, the full request/response
  contract, and the refusal are all real; only execution is not.
- **2026-08-25, ADR-0126**: Constitution proposals land with the same shape
  as Design -- a free-text `author` field, `withdraw_proposal` gated by a
  bare string-equality check against it. ADR-0126's own consequences section
  names this precisely: "Attribution of the *author* of a Bridge-originated
  proposal is honest but weak until ADR-0098/OIDC lands: a label, not a
  verified principal."
- **2026-08-26, ADR-0128**: Refuses to accept that OIDC is the only way to
  get a "verified principal," for a reason specific to self-hosted,
  single-tenant Ackplane (ADR-0088's Compose topology, or an equivalent
  single-operator install): the hardened loopback token (`development_tenant_token
  = hex(SHA-256(salt || tenant_name))`, ADR-0098 decision 3) is proof of
  possession of a file on the machine the operator deployed Ackplane onto --
  "not a synonym for no principal." It resolved this specifically for
  Administration's four privileged classes (snapshot, export, purge,
  recovery inspection), and said nothing about Work, Design, or Constitution,
  because none of those ADRs were in its scope.

Read together as of 2026-08-29, this repository holds three different,
contradictory answers to the identical question -- "does the loopback
profile identify anyone real?" -- for the identical trust boundary (one
operator, one machine, one Compose deployment): **yes** for Administration
(ADR-0128), **no, permanently** for Work commands (ADR-0125 decision 2), and
**the question was never asked** for Design and Constitution (ADR-0123,
ADR-0126 -- both predate ADR-0128 and could not have applied its answer).
The practical effect: the Bridge Work command routes built on ADR-0125 (this
session's own `work_command_api/`) are fully wired, fully tested, and
permanently inert for the single-operator deployment ADR-0128 already
decided has a real verified principal available -- not because anyone
decided execution should stay disabled, but because no one had yet noticed
the two ADRs disagreed. Design and Constitution mutations, meanwhile, accept
whatever identity string a caller supplies, which is exactly the gap ADR-0128
closed for Administration receipts one day later and never revisited here.

This ADR is authoring, not implementing: per this repository's own
discipline, a Proposed ADR authorizes no code change. It exists so an
implementing agent has one settled design to build against instead of
re-deriving it, or re-discovering the ADR-0123/ADR-0125 contradiction, from
scratch.

## Decision

**The hardened loopback profile (ADR-0098 decision 3's salt-derived tenant
token) is the verified principal ADR-0125 decision 2 requires for Work
commands, and the accountable identity Design mutations and Constitution
proposals record, under exactly the same self-hosted single-tenant scope
ADR-0128 already drew. Multi-tenant Ackplane is unchanged: it still waits for
ADR-0098 decision 4's OIDC, exactly as ADR-0128 left it.**

1. **Scope is identical to ADR-0128's, not reargued.** This ADR adds no new
   trust boundary. `BridgeConfig::resolve`'s existing refusal of a
   non-loopback bind without a production verifier (ADR-0094) is still the
   single enforcement point: the moment a deployment stops being the
   single-operator loopback shape both this ADR and ADR-0128 are scoped to,
   it already refuses to start under the looser basis, so none of the
   following can reach a production multi-tenant deployment by accident.

2. **Work commands: the salted tenant token resolves to
   `WorkCommandAuthorization::Verified`, not `LoopbackDevelopment`, for a
   self-hosted deployment.** `VerifiedWorkCommandPrincipal` is constructed as:
   - `principal_id`: the salted `development_tenant_token` itself -- the same
     value `AdministrationApiState`/`ConstitutionApiState` already record as
     the accountable identity on every other privileged receipt. Not a
     placeholder; the actual identity for a deployment with exactly one
     operator, exactly as ADR-0128 decision 1 already reasoned for
     Administration.
   - `tenant_id`: the Bridge's own resolved tenant (`state.tenant_id`),
     matching every other route.
   - `repository_ids`: resolved from the Bridge's own enrolled-repository
     visibility check (`ensure_repository_visible`), already performed before
     every Work command route executes -- the same reachability boundary
     every other Bridge route already enforces, not a new lookup.
   - `allowed_commands`: the full closed ten-command vocabulary. No basis
     exists yet to grant a self-hosted single operator a *subset* of their
     own deployment's commands, and inventing one would be authorization
     theater over a boundary ADR-0094 already enforces structurally.
   - `delegation_id`: `None`. A Bridge-originated Work command under this
     profile is a **direct verified human request**, recorded as such
     (decision 2's own distinction) -- never silently rewritten as an
     ADR-0115 agent delegation, and never requiring one, because no
     automated requester is involved.
   - `policy_refs`: empty, deliberately -- see decision 5 below for why Work
     commands do not gain an adopted-policy layer the way Administration
     did.

3. **Confirmation stays exactly as ADR-0125 decision 8 specified.** This ADR
   changes *who* is asking, never the machinery around *how* a consequential
   command executes: a command still produces an immutable preview and
   payload digest, the caller still confirms that exact digest before a
   short expiry, and a changed field, task version, or authorization basis
   still forces a new preview. `CreateWork` may still skip the second
   confirmation only when a verified policy classifies it as routine (decision
   8's own exception) -- which, absent decision 5's policy layer, means every
   `CreateWork` under this profile takes the confirmed path until a future
   policy decision says otherwise.

4. **Design mutations and Constitution proposals record the verified
   principal as the authoritative actor/author, not a caller-supplied
   string.** `propose_design`, `record_design_decision`,
   `record_design_materialization`, and `propose_clause` stop trusting the
   HTTP body's `proposed_by`/`actor`/`author` field as identity. The
   authoritative, receipted identity becomes the same salted tenant token
   Work commands and Administration already use; a caller may still supply a
   bounded, optional *display label* (e.g. "who to show in the UI"), stored
   separately from and never substituted for the verified principal.
   `withdraw_proposal`'s author-gate compares against the verified principal,
   not the free-text label -- closing the exact gap ADR-0126 named as
   "honest but weak." This is a strictly narrowing change: every existing
   caller of these routes already runs under the loopback profile, so no
   currently-valid request stops being attributable; it becomes
   *un-forgeable* rather than merely labeled.

5. **Work commands do not gain an adopted-policy layer analogous to
   `AdministrationPolicy`, and this ADR explains why rather than leaving it
   implicit.** Administration's four classes (snapshot, export, purge,
   recovery) are each irreversible or off-band by nature: a purge deletes
   rows, a snapshot/export copies a tenant's data outside the ledger
   entirely, so ADR-0119 decision 2 requires a durable, pre-committed
   authorization record independent of the moment a caller acts. Work
   commands differ in kind, not just degree: the five server-owned kinds
   are ordinary, reversible Work/Claim state moves already visible in
   `work_task_history`'s own append-only event stream (a released lease can
   be reclaimed; a reviewed task can be reviewed again), and the five
   supervisor-directed kinds already carry a *second*, independent
   authorization layer Administration has no equivalent of: ADR-0107's
   directive/receipt chain, which refuses a directive naming a capability
   the addressed supervisor never declared, before any receipt claims
   otherwise. Requiring a second, Administration-shaped policy record on top
   of a verified principal, a confirmation digest, and (for five of the ten
   kinds) a capability-checked directive receipt would add ceremony without
   adding a safety property this ADR can name. If a future need for
   per-operation Work policy emerges (for example, bounding which
   repositories or task scopes an operator's confirmations may reach), that
   is its own reviewed decision, not assumed here.

6. **Attribution is real, not merely renamed.** Every Work command receipt,
   Design decision, and Constitution proposal now names the verified
   principal that requested it, the same durable, resolvable identity
   Administration receipts already carry. A future OIDC principal (ADR-0098
   decision 4) does not need a schema change to slot in beside it: the
   verified-principal shape (`principal_id`, `tenant_id`, scope) is already
   what every one of these authorization checks consumes; only how
   `principal_id` gets resolved changes.

## Consequences

- The Bridge Work command routes (`work_command_api/`) built and merged
  under ADR-0125 become genuinely operable for a self-hosted, single-tenant
  deployment for the first time, without any change to their wire contract,
  tests, or the routes themselves -- only the authorization value
  `submit_work_command`/`confirm_work_command` construct changes from
  `LoopbackDevelopment` to `Verified`.
- Design and Constitution mutation receipts stop recording a self-asserted
  identity string as if it were accountable, closing a real, previously
  undesigned gap in both ADR-0123 and ADR-0126 -- not a new capability, a
  correction to an attribution ADR-0126 itself already called weak.
- Nothing here grants any new capability to a multi-tenant deployment; the
  ADR-0094 refusal that already gates non-loopback binds is unchanged and
  remains the single enforcement point.
- An implementing agent still has to: thread a resolved `VerifiedWorkCommandPrincipal`
  (or equivalent) through `WorkCommandApiState`/`submit_work_command`/
  `confirm_work_command` rather than the hardcoded `LoopbackDevelopment`
  value; add the same principal-resolution step to `design_api.rs`'s three
  mutation handlers and `propose_clause`/`withdraw_proposal`; and write the
  regression tests this repository requires for a bug/gap fix (a command
  that used to always refuse now executes under the loopback profile; a
  proposal author is now the verified principal, not the caller-supplied
  string). None of that is done by this ADR.
- `docs/ARCHITECTURE.md`'s Work-command and Design/Constitution paragraphs
  will need a further update once implemented, to stop describing
  `authorization_unavailable` as the only reachable outcome and stop
  describing Design/Constitution attribution as unverified.

## Rejected alternatives

**Wait for ADR-0098 decision 4's OIDC before enabling any of these.** This is
the status quo this ADR replaces. Rejected for the same reason ADR-0128
rejected it for Administration: it treats "verified principal" as a synonym
for "OIDC-authenticated human," manufactures a wait for infrastructure
ADR-0098 itself said not to build speculatively, and leaves a real,
already-built, already-tested command surface permanently inert for the
single-operator deployment this product's own near-term shape actually is.

**Give Work commands their own, narrower loopback-principal story instead of
reusing ADR-0128's.** Rejected: it would be the same argument made twice
with two chances to disagree with itself, which is exactly the failure mode
that produced the ADR-0123/ADR-0125/ADR-0126/ADR-0128 contradiction this ADR
exists to close. One precedent, applied consistently, is easier for the next
reader and the next ADR to reason about than four adjacent but
independently-reasoned ones.

**Add an `AdministrationPolicy`-style adopted policy for Work commands too,
for uniformity with Administration.** Rejected in decision 5 above on the
merits (Work commands are reversible and, for half their vocabulary, already
carry a second capability-checked authorization layer Administration has no
equivalent of) rather than for uniformity's own sake, which is not a safety
property.

**Let Design/Constitution mutations keep trusting the caller-supplied
identity string, since ADR-0123 already reviewed and accepted that
shape.** Rejected because ADR-0123's own acceptance was explicit that
per-operator identity "remains deferred," not settled as sufficient
indefinitely, and ADR-0126's own consequences section already flagged the
same gap as weak. Leaving it unresolved after ADR-0128 established a real
answer to the identical question would be choosing not to apply a decision
already made, not choosing a different one.
