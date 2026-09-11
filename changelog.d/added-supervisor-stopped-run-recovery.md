- Added explicit supervisor `recover inspect` and digest-confirmed `recover confirm`
  commands for accounted-for stopped workers. Versioned run markers and atomic
  adapter-stop provenance bind the original node, session, directive and workspace
  to terminal evidence. Cleanup uses the enrolled companion to replay exact
  pending receipts and release only the original owner, records completion before
  marker removal, and resolves retries after a lost response. Live/unproven runs,
  changed or unsupported evidence, missing server positions and permanent refusals
  retain their marker; no process is spawned or signalled and Work is not completed.
