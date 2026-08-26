- **The engine was ungoverned and the gate that would enforce it cannot run —
  first half CLOSED within the day, second half still OPEN.**
  `ARCHITECTURE.md` calls the conformance chain "the only trustworthy proof that
  the agents did the sanctioned work". Measured twice from a live
  `export_conformance_manifest`, roughly seven hours apart:

  | | 03:37Z | 09:29Z | 31 Jul |
  |---|--:|--:|--:|
  | governed code nodes in the whole workspace | 8 | **161** | **163** |
  | of those, `.rs` under `crates/` | 0 | **133** | **135** |
  | receipts covering zero governed nodes | 127 of 131 (97%) | **72 of 172 (42%)** | see note |

  At the first measurement the eight were `.pre-commit-config.yaml` and seven
  script files: the entire engine was ungoverned, so a receipt reading `aligned`
  most often meant "there was nothing to check" rather than "the work was
  proven". That is no longer true, and the entry is corrected rather than
  deleted because the shape of the mistake is worth keeping — the first figure
  was right when taken and wrong within hours, and a Known gap that reports a
  measurement without its timestamp will keep being read as current long after
  it stops being. The 31 Jul column is from `goal_code` directly; the receipts
  ratio is deliberately not restated there, because it is a ratio over a
  manifest this entry's own second half says cannot be exported, and a figure
  derived from another source would look comparable without being so.

  — *Why this one went stale so fast, which generalises.* The original entry
  closed by calling the coverage half "~30 goals' worth of attributed judgement"
  and "not an agent's to make quietly" — it deferred to a human. Somebody then
  did it, and nothing told the fragment, so the deferral became the record. **An
  entry that defers to a human decision is among the likeliest to rot**, because
  whoever acts on it is not whoever maintains the catalogue and the act leaves no
  trace in the file that asked for it. That is the opposite of the intuition that
  an alarming entry gets watched closely. A duplicate of this entry
  (`the-conformance-chain-governs-8-code-nodes-none.md`) survived until 31 Jul
  and was independently re-measured to the same conclusion before anyone noticed
  the two were the same gap; it has been folded in here.
  **Re-measure before quoting either number: `node scripts/binding-audit.mjs`
  reports per-directory coverage and names the unbound files.** As of 09:29Z it
  reports 131 of 136 source files bound, with five unbound: `db/repairs.rs`,
  `store/events.rs`, `graph/repair/collapse.rs`, `ingest/structure/rust.rs`, and
  `mindleak-storage/src/build_identity.rs`. Every one is a file added recently,
  which is the residual gap: a binding is applied to the tree as it was, and a
  new module arrives ungoverned and silent.
  **24 Aug re-measurement, second confirmation of the same residual gap.**
  `binding-audit.mjs` reported 0 of those five unbound (all long since folded
  into other work) but a completely DIFFERENT five: `bin/register-me/main.rs`,
  `claim_store/mod.rs`, `enrollment_service/wire.rs`, `fleet/repositories.rs`,
  and `service/mod.rs` — every one a sibling file created by a module-length
  split of an already-governed file (`claim_store.rs`, `enrollment_service.rs`,
  `fleet.rs`, `service.rs`; the split leaves the original binding pointing at a
  path that no longer exists rather than following the file to its new home).
  Bound via `constitution_define(bind)` the same session (task:eb66335f3264);
  `binding-audit.mjs` now reports 0 unbound and 18 stale bindings naming the
  pre-split paths (left as-is — cleaning up a stale binding is not the same
  gap as leaving a file unbound, and wasn't this task's acceptance). The
  pattern repeats exactly as predicted: this is not a one-time fix, it is a
  standing tax on every future split, and nothing currently binds a file
  automatically at split time.
  **Later same day, third confirmation, both halves of the residual closed
  for now.** `binding-audit.mjs` had drifted again: 1 newly unbound file
  (`ackplane-bridge/src/shared_assets.rs`, added since the last measurement)
  and 17 stale bindings naming pre-split paths across `lodestar-core`,
  `lodestar-mcp`, `mindleak-core`, and `ackplane-server`
  (`facade/executive.rs`, `tools/executive.rs`, `tools/design.rs`,
  `facade/constitution.rs`, `model/executive.rs`, `ingest/ast.rs`,
  `telemetry.rs`, `projection.rs`, `service.rs`, `bin/register-me.rs`,
  `claim_store.rs`, `enrollment_service.rs`, `enrollment_store.rs`,
  `fleet.rs`, `constitution_store.rs`, `supervisor_store.rs`,
  `design_store.rs` — every one confirmed, before touching anything, to
  already have its real successor file(s) bound to the same goal via
  `constitution_query(governing, ...)`, so this was pure dangling-reference
  cleanup, never a coverage gap). Fixed the new file with `bind`, removed all
  17 stale rows with `unbind`. `binding-audit.mjs` now reports **0 unbound, 0
  stale, 0 stranded** — the first time this fragment's own re-measurement has
  found nothing left to fix. Expect this to drift again the next time a file
  is added or a module is split; re-run `node scripts/binding-audit.mjs`
  before trusting either number, exactly as this fragment keeps saying.
  **2026-08-26, fourth confirmation, pattern repeats exactly as predicted.**
  `binding-audit.mjs` reported 0 unbound but 8 stale bindings, all naming a
  deleted `ackplane-server/src/work_command_store/{execute,model}/*.rs` split
  that has since been re-consolidated back into flat `mod.rs`/`write.rs`/
  `service.rs`/`model.rs` files. Confirmed via `constitution_query(governing,
  ...)` on each of those four real files before touching anything: all four
  already carry `goal:ackplane-federation-service@constitution:v4`, the same
  goal the 8 stale rows named, so this was pure dangling-reference cleanup
  again, never a coverage gap. Removed with one `unbind` call, no `bind`
  needed since nothing was actually unbound this time. `binding-audit.mjs`
  again reports **0 unbound, 0 stale, 0 stranded**. Four occurrences in one
  gap fragment is itself the data point: this is not a one-time cleanup, it
  is a standing tax on every module split *and* every re-consolidation, and
  the fix that would end the recurrence — binding following a file
  automatically when it moves — remains unbuilt.
  **Still true, and re-verified 09:29Z: `scripts/conformance-gate.mjs` cannot
  run.** It reads the manifest exported by `export_conformance_manifest`, and
  `.gitignore` still excludes `/.lodestar/*` with a single exception for
  `CONSTITUTION.md`. Nothing is tracked under `.lodestar/`, and the gate still
  appears in no workflow, no Makefile target, and no hook — not by oversight but
  by construction. Anyone "wiring it into CI" will find there is nothing for it
  to read. Making it runnable means deciding whether a regenerable,
  agent-produced proof artifact belongs in Git; that is a call for the
  maintainer, not an agent.
  The gate does at least no longer *report* a pass it did not earn: it used to
  print `OK — N changed path(s), no governed gaps` whether it had verified
  everything or nothing, and it now says `CHECKED NOTHING` when no changed path
  was in scope — the same correction already applied to receipts that were
  `aligned` over an empty bundle.
