- **A repository now declares which arbiter owns its coordination.** Set
  `MINDLEAK_COORDINATION_MODE` to `local` — the default, and what every existing
  repository already is — or to `federated`. Both planes resolve it once at
  startup rather than per call, because an authority that can move between calls
  is two authorities. No Ackplane client ships yet, so declaring `federated` is
  refused with an explanation rather than quietly downgraded to local
  arbitration, which would hand the repository a second arbiter
  ([ADR-0082](../docs/adr/0082-ackplane-is-a-standalone-federation-service.md),
  [ADR-0045](../docs/adr/0045-a-fleet-is-a-distributed-system.md)).
