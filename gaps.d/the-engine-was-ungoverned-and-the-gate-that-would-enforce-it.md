- **The engine was ungoverned and the gate that would enforce it cannot run —
  first half CLOSED within the day, second half still OPEN.**
  `ARCHITECTURE.md` calls the conformance chain "the only trustworthy proof that
  the agents did the sanctioned work". Measured twice from a live
  `export_conformance_manifest`, roughly seven hours apart:

  | | 03:37Z | 09:29Z |
  |---|--:|--:|
  | governed code nodes in the whole workspace | 8 | **161** |
  | of those, `.rs` under `crates/` | 0 | **133** |
  | receipts covering zero governed nodes | 127 of 131 (97%) | **72 of 172 (42%)** |

  At the first measurement the eight were `.pre-commit-config.yaml` and seven
  script files: the entire engine was ungoverned, so a receipt reading `aligned`
  most often meant "there was nothing to check" rather than "the work was
  proven". That is no longer true, and the entry is corrected rather than
  deleted because the shape of the mistake is worth keeping — the first figure
  was right when taken and wrong within hours, and a Known gap that reports a
  measurement without its timestamp will keep being read as current long after
  it stops being.
  **Re-measure before quoting either number: `node scripts/binding-audit.mjs`
  reports per-directory coverage and names the unbound files.** As of 09:29Z it
  reports 131 of 136 source files bound, with five unbound: `db/repairs.rs`,
  `store/events.rs`, `graph/repair/collapse.rs`, `ingest/structure/rust.rs`, and
  `mindleak-storage/src/build_identity.rs`. Every one is a file added recently,
  which is the residual gap: a binding is applied to the tree as it was, and a
  new module arrives ungoverned and silent.
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
