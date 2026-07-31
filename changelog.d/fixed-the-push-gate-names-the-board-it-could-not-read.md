- **The push gate now names the failure it actually hit instead of always
  blaming an unreachable ledger.** `canonical-push` collapsed three distinct
  conditions — the Lodestar binary did not answer, it answered but did not
  identify the session, and it answered and identified the session but its task
  board could not be read — into a single "the ledger is unreachable" refusal
  that offered `cargo build`. The board-read case is the common one: a deployed
  binary older than the ledger cannot parse an event a newer writer recorded, so
  the remedy is a current binary (point `LODESTAR_MCP_BIN` at the shared
  install), not a rebuild of a ledger that is answering. Each condition now
  refuses with its own cause and its own remedy.
