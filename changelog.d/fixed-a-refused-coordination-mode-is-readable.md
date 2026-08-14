- **A refused coordination mode is now readable instead of killing both planes
  in silence.** `resolve_coordination_mode` is the first call in either MCP
  binary, and its error propagated out of `main`. So a misspelt
  `MINDLEAK_COORDINATION_MODE` (`cloud`), or `federated` declared before a
  client exists, killed the process before it could serve anything: the agent
  saw only a server that failed to start, and because one variable stops both
  planes identically, the operator saw "everything is broken" rather than "one
  declaration is wrong". The refusal itself was never in doubt — it was correct
  and carried the remedy — it simply arrived somewhere nobody reads.
  Both binaries now stay up and answer the protocol, refusing every `tools/call`
  with that reason. Refusing is not downgrading: a process that arbitrates
  nothing cannot become the second arbiter ADR-0082 forbids, so the guarantee
  that exiting provided is unchanged. The notice also states that no task,
  claim, lease, or evidence was touched, because an agent that cannot tell
  whether a refusing process half-did something has to assume it did.
