- **Fixed the Ackplane one-shot migration runner to apply Evidence schemas.**
  Fresh Compose deployments now create Evidence and conformance tables before
  `EvidenceService` starts, including finding-code and Constitution-version
  columns needed by the Bridge Evidence Board.
