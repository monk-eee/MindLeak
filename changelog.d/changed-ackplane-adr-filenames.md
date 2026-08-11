- **The Ackplane ADR files are now named Ackplane on disk.** Six decisions
  carried titles naming *Ackplane* while their filenames still said
  *backplane* — `0082`, `0083`, `0084`, `0086`, `0087` and `0088` — so the
  generated index rendered rows like
  `0083-grpc-is-the-backplane-node-protocol.md | gRPC is the Ackplane node
  protocol`, and searching the repository for the product name did not find the
  documents that define it. The files are renamed with their history intact and
  every cross-reference updated, including the README pointer and the generated
  ADR index. No decision, status, or body text changed. ADR-0089's phrase "the
  passive bus a backplane would be" is deliberately untouched: that is the
  common noun making the argument, not the product name (ADR-0089 §5).
