- **Released MCP server binaries can now actually federate, as their own error
  message has always told operators they could.** `MINDLEAK_COORDINATION_MODE=
  federated` is refused when a build carries no Ackplane client, with the remedy
  *"install a build that includes the client"* — but every release was built
  without the `federation-client` feature, so no published binary had ever
  contained one. The federated claim path (Ed25519-signed lease, renew, release,
  recover, backed by the OS credential facility) was written, reviewed and
  tested against real PostgreSQL, then compiled into nothing a user could
  obtain, which left Ackplane coordination reachable only from a hand-built
  checkout. Release and CI now build both servers with that feature.
  Local coordination is unchanged: the Ackplane client is only constructed when
  the resolved mode is `federated`, so a Local install still needs no account,
  network, PostgreSQL or Docker — it costs 1.4 MB on `mindleak-mcp` and 2.1 MB
  on `lodestar-mcp`. The refusal now names remedies that exist: reinstall from a
  release, or build with `--features federation-client`.
