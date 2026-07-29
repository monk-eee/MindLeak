- **The fifteen-tool design cluster is now four verbs.** Registering a design,
  deciding it, promoting it and reading the ledger each had their own tool
  name — fifteen entries on the surface for four things an agent actually
  does, and an agent choosing between `reopen_undecided_design` and
  `attribute_design_decision` had to know which one its row belonged to before
  it could ask. They are now `design_register`, `design_decide`,
  `design_promote` and `design_query`, with the act named as an argument
  (`decision`, `step`, `view`). Nothing was relaxed to make them fit: every
  refusal a separate name used to encode is now argument validation carrying
  the same message, and the ADR-0051 guards survive intact — `attribute` still
  refuses to overwrite a `decided_by` the ledger already holds, and `reopen`
  still defers to materialisation, refusing a row whose promotion has created
  work. The two therefore continue to partition the undecided rows rather than
  overlap. The old fifteen names still answer for one minor version, and each
  reply names the call to make instead, so the deprecation teaches rather than
  merely failing; removal ships with the release train named in ADR-0059.

- **The guard that checks the server advertises everything it answers to was
  reading its own source.** `every_tool_the_server_answers_to_is_advertised`
  scans dispatch blocks by searching for the text `match name {` — and
  `mod.rs`, which delegates to every other module and dispatches nothing
  itself, contains that text only inside the test's own search call. The scan
  therefore treated the rest of its own file as if it were tool dispatch. It
  now reads only the modules that answer to a name, and only the arms of the
  dispatch itself rather than the nested matches that parse arguments, since
  reporting an argument value as an unadvertised tool trains people to ignore
  the guard. Because that narrowing is exactly the kind that can stop reading a
  module without failing anything, the test now asserts it found dispatch in
  every module it claims to cover, not merely in total.
