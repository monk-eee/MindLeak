# ADR-0150: Delegated claim owners are bound to the authenticated node

- Status: Proposed
- Date: 2026-09-14
- Related: [ADR-0096](0096-ackplane-arbitrates-federated-claims-through-leased-delegation.md),
   [ADR-0100](0100-repository-node-owns-one-non-exporting-signer.md),
  [ADR-0111](0111-bridge-recovers-a-stranded-claim-as-a-tenant-scoped-administrative-action.md)

## Evidence

A real gRPC regression enrolled two independent signing keys in one repository.
Node A obtained a live lease as `owner-a`. Node B signed a release with its own
valid key while naming `owner-a`; the server returned `released: true`.
Authentication verified the key, signature, nonce and repository, but the claim
row only recorded an owner string. It could not distinguish that owner on two
different nodes. A neighboring context regression also accepted a lease whose
session name matched while its node differed.

This record describes the node-custody correction for review. Proposed status
does not adopt a new deployment profile, expand administrator authority, or
certify the Industrial stabilization programme.

## Decision

1. A delegated owner is the pair `(owner_id, owner_node_id)` within its existing
   tenant, repository and task. Node identity comes from verified request
   authentication, never another caller-supplied wire field. The gRPC messages
   and signing-byte contract do not change.
2. The durable compare-and-swap owns authorization. `delegate` persists the node
   with a granted claim. `release`, `renew`, `park`, and `answer` require the
   matching node when addressing that owner. A valid peer signature cannot turn
   knowledge of an owner ID into custody. Mismatched or unknown node custody
   returns `permission_denied` without changing the grant.
3. Checks and writes share the claim row lock. Concurrent nodes requesting the
   same owner label cannot both become its live custodian. Existing different-owner
   arbitration, expiry boundaries, parked-state exclusions and nonce checks
   remain in force.
4. Only the same owner and node preserve `claim_started_at` and branch on
   reacquisition. After expiry, an otherwise eligible different-node grant starts
   a new window even if its owner label is unchanged. Explicit recovery still
   requires the expected owner and reason and refuses a live or parked claim.
5. Mutation history records the requested owner's node with the existing decision.
   Current context compilation requires task, session owner and node to match
   the live lease, both before and after compilation. Historical context-use
   receipts retain their existing session/node checks and replay semantics.
6. The existing tenant-scoped Bridge administrator can recover an expired claim
   to an explicitly named next owner and node. Its request/form now require
   `node_id`, and the response reports it. Naming a node is a destination choice,
   not authentication of that node; later node operations still require its
   enrolled key. The Bridge is not a new node-signing fallback.

## Migration And Upgrade

Migration 67 adds nullable `owner_node_id` to current claims and nullable
`requested_node_id` to history. It does not edit prior migrations, infer node
identity from a label or nonce, or backfill authority into older rows. Old
history and all other claim values are retained.

Resolve parked claims and stop/drain active supervisors before upgrading the
server and Bridge together. Stop old claim writers during the migration; an old
binary does not implement this custody check. Apply the normal reviewed
migration procedure to the deployment, then restart with consistent artifacts.
Do not run branch-local migrations against live or shared databases.

A surviving unbound legacy claim is not silently adopted by renew, release,
park, answer, or same-owner live delegation. An unparked claim can be freshly
acquired or explicitly recovered after expiry, recording a new node and a new
window. A parked unbound claim requires operator investigation under a reviewed
recovery procedure; this change does not invent the missing provenance or
bypass the parked-state guard. Do not edit a node value into the database merely
to make an owner mutation pass.

Existing Bridge recovery clients must include the destination `node_id`; a
missing field is refused, not filled from the previous owner or process.
Same-node key rotation uses the existing key lifecycle checks and does not
change the node-bound claim identity.

## Verification

- Confirmed the signed peer-release and cross-node context regressions fail
  against the uncorrected code and pass with node custody enforced.
- Signed gRPC tests cover both nodes' legitimate work, peer release/renew/park/
  answer/delegate refusal, parked-owner refusal and live recovery refusal.
- Store tests cover concurrent same-label claims, unchanged denied grants,
  expired same-label custody transfer, unknown legacy custody and idempotent
  migration without backfilling old rows.
- The real Bridge handler test covers required destination-node input, durable
  custody, live-lease refusal and tenant isolation. Browser checks verify the
  required field and payload on desktop and mobile.

This is cooperative leased ownership, not process sandboxing or proof that an
expired worker has stopped. Full two-node packaged-runtime and production
authentication qualification remain separate gates.
