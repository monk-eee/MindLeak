- **The publication record says why it could not certify, and how to fix it.**
  A push that could not record its commit in the Memory Plane reported one line
  — "the Memory Plane was unreachable" — for three different causes: no
  `mindleak-mcp` binary, an unregistered session id, and a rejected write. The
  most common cause by far is the most misleading one: a linked worktree has no
  `target/` of its own, so the resolver finds no binary and an operator reads an
  outage where a single environment variable was missing. Each cause now has its
  own notice, and the missing-binary one names the remedy the way the Lodestar
  gate beside it already did.
