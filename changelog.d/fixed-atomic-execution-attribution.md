- **Atomic execution evidence:** execution facts and their agent observation now
  commit together, preventing concurrent maintenance from reaping a newly
  committed execution before attribution and causing a foreign-key failure.
  An attribution error rolls back the whole batch instead of leaving partial
  evidence. Deterministic ids, fact counts, observation half-lives and ordinary
  decay remain unchanged; no retry or retention workaround is added.
