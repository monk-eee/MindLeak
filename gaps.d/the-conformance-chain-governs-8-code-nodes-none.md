- **The conformance chain governs 8 code nodes, none of them Rust, and the gate
  that would enforce it cannot run — MEASURED, partially mitigated.**
  `ARCHITECTURE.md` calls the conformance chain "the only trustworthy proof that
  the agents did the sanctioned work". Measured 2026-07-29 from a live
  `export_conformance_manifest`:

  | | |
  |---|--:|
  | governed code nodes in the whole workspace | **8** |
  | of those, files under `crates/` | **0** |
  | receipts covering zero governed nodes | **127 of 131** |
  | verdicts | 52 aligned · 12 drift · 67 needs_human |

  The eight are `.pre-commit-config.yaml` and seven `scripts/` and
  `editors/vscode/scripts/` files. The entire engine — `mindleak-core`,
  `lodestar-core`, both MCP servers — is ungoverned, so 97% of receipts prove
  nothing about any governed code, and a receipt reading `aligned` most often
  means "there was nothing to check" rather than "the work was proven".
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
  Not fixed, because both halves are decisions rather than patches: binding the
  engine to goals is ~30 goals' worth of attributed judgement, and making the
  gate runnable means deciding whether a regenerable, agent-produced proof
  artifact belongs in Git. Either is a reasonable call; neither is an agent's to
  make quietly.
