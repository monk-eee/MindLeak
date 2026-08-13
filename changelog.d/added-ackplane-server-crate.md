- **Ackplane's service side exists as `crates/ackplane-server`.** ADR-0082 makes
  Ackplane a separately deployable arbiter rather than a mode of either plane, and
  until now only the repository side of that boundary was built — which left the
  ledger, the graph projection and the container topology all describing work with
  nowhere to live. The crate depends on no plane crate, and what it carries so far
  is the startup contract: without `ACKPLANE_DATABASE_URL` it refuses to start,
  because an instance holds no authoritative local state and cannot accept work
  without the ledger (ADR-0086 clause 1), and failing later would disguise a
  misconfiguration as a fault in whichever request arrived first. It binds loopback
  by default and prints a banner naming its profile (ADR-0088 clause 6), and a
  `quorum_durable` claim is refused unless `ACKPLANE_SYNCHRONOUS_STANDBYS` names the
  failure domain it rests on — a durability claim nothing backs is precisely the
  asynchronously replicated acknowledgement ADR-0086 clause 12 will not label as
  zero-loss. The database URL carries a password, so it appears in neither the
  banner nor the hand-written `Debug`. The process exits rather than listening,
  since accepting work it cannot durably record would be the dual authority
  ADR-0082 clause 3 refuses.
