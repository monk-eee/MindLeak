- **The Lodestar default tool profile is saturated at ~5,997 of its 6,000-token
  budget, so a correct schema no longer fits.** Observed while fixing
  `constitution_define`'s `records` property, which advertised a bare
  `{"type": "array"}` with no `items` and made strict MCP clients reject the
  entire server (`tool parameters array type must have items`). Where:
  `crates/lodestar-mcp/src/tools/constitution.rs:definitions`, measured by
  `crates/lodestar-mcp/src/tools/mod.rs::the_default_profile_is_under_budget`
  (ADR-0059 rule 2). Impact: the *minimum* valid `items` schema costs 25 bytes
  more than the profile has, so the fix had to be paid for by trimming the
  tool's own description, and the import record shape (`external_id`, `kind`,
  `title`, `statement`, `status`, `source_ref`, `source_digest`) cannot be
  advertised at all — callers discover it only from `ExternalGoalRecord`'s
  deserialization error. The next legitimate schema addition to any
  default-profile tool hits the same wall with no prose left to sell. The
  structural question ADR-0059 leaves open: `import` is specialist by its own
  reasoning, but a tool has one schema across profiles by design, so the common
  path pays for specialist arguments. Fixed this run: only the validity defect
  and enough budget to land it; the saturation itself is left for a deliberate
  decision.
- **PORTABLE: an invalid advertised schema fails whole-surface, not per-field.**
  One malformed property in one tool's JSON Schema made every other tool on the
  same server uncallable, and the client error named the shape
  (`array type must have items`) without naming the tool or the property. A
  schema surface is only as loadable as its worst entry, so it is worth a test
  that walks every advertised schema for structural validity rather than
  asserting on the fields any one tool happens to declare.
