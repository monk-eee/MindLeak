- **What**: `crates/ackplane-bridge/tests/repository_id_guard.rs`'s
  `HANDLER_SOURCES` list (the source files it statically scans for a
  tenant-scope check) only covers the older `src/handlers/repository/*.rs`
  split-handler modules. It does not include any of the newer flat
  `src/<domain>_api.rs` modules -- `work_api.rs`, `knowledge_api.rs`,
  `delegation_api.rs`, `context_api.rs`, `evidence_api.rs`,
  `supervisor_api.rs`, `live_feed.rs`, `administration.rs`, or the new
  `design_api.rs` (ADR-0121/ADR-0123). `every_bridge_route_handler_scopes_its_query_to_the_tenant`
  therefore passes for those modules by omission, not by actually checking
  them.
- **Where**: `crates/ackplane-bridge/tests/repository_id_guard.rs`, the
  `HANDLER_SOURCES` const and its `include_str!` list near the top of the
  file.
- **Impact**: the guard's own doc comment claims "every Bridge route
  handler... must carry an explicit tenant scope," checked structurally --
  but roughly half of the crate's route handlers, including all three new
  Design mutations, are invisible to it. A future handler that forgets
  `state.tenant_id` in one of the unlisted modules would not be caught by
  this test.
- **Not fixed this run**: out of scope for the Design interactivity change
  that found it -- fixing this properly means auditing every `*_api.rs`
  module's handlers against the guard's existing exemption lists
  (`ROUTE_HANDLERS_WITHOUT_A_STORE_QUERY` etc.), which is its own
  self-contained task, not a one-line addition.
