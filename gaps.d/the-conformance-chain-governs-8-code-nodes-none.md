- **The engine is now largely governed; the gate that would enforce it still
  cannot run — MEASURED 2026-07-29, RE-MEASURED 2026-07-31, half fixed, OPEN on
  the gate.**
  `ARCHITECTURE.md` calls the conformance chain "the only trustworthy proof that
  the agents did the sanctioned work". As first measured on 2026-07-29 from a
  live `export_conformance_manifest`:

  | | 29 Jul | 31 Jul |
  |---|--:|--:|
  | governed code nodes in the whole workspace | **8** | **163** |
  | of those, files under `crates/` | **0** | **135** |

  — *The coverage half is fixed, and the original entry understated it by 20x
  within two days.* The eight were `.pre-commit-config.yaml` and seven
  `scripts/` and `editors/vscode/scripts/` files, and the entry concluded that
  "the entire engine — `mindleak-core`, `lodestar-core`, both MCP servers — is
  ungoverned", so that a receipt reading `aligned` most often meant "there was
  nothing to check". That is no longer true: `goal_code` now binds 163 nodes, 135
  of them under `crates/`, across both engines and both MCP servers. The entry
  called this half "~30 goals' worth of attributed judgement" and not an agent's
  call to make quietly; somebody made it. The receipts-covering-nothing figure is
  deliberately not restated here, because it was a ratio over a manifest this
  gap's own second half says cannot be exported — re-derive it when the gate can
  run.
  **`scripts/conformance-gate.mjs` cannot close this, because it cannot run.**
  It reads the manifest exported by `export_conformance_manifest`, and
  `.gitignore` excludes `/.lodestar/*` with a single exception for
  `CONSTITUTION.md`. The artifact it needs is by policy never committed, so the
  gate appears in no workflow, no Makefile target, and no hook — not by
  oversight but by construction. Anyone "wiring it into CI" will find there is
  nothing for it to read.
  Mitigated here only in that the gate no longer *reports* a pass it did not
  earn: it used to print `OK — N changed path(s), no governed gaps` whether it
  had verified everything or nothing, and it now distinguishes the two, saying
  `CHECKED NOTHING` when no changed path was in scope. That is the same
  correction already applied to receipts that were `aligned` over an empty
  bundle — agreement about nothing reported in the words of proof.
  Not fixed, because the remaining half is a decision rather than a patch:
  making the gate runnable means deciding whether a regenerable, agent-produced
  proof artifact belongs in Git. That is a reasonable call either way, and not
  an agent's to make quietly. Re-verified 2026-07-31 — `.gitignore` still carries
  `/.lodestar/*` with the single `CONSTITUTION.md` exception, `git ls-files
  .lodestar` returns nothing, and `conformance-gate` appears in no workflow, no
  Makefile target and no hook.
