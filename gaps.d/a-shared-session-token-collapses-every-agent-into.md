- **A shared session token collapses every agent into one identity — GUARDED,
  not prevented.** — `LODESTAR_SESSION_ID` is minted by the client, so a token
  written anywhere several agents read (repository memory, a dotfile, a prompt)
  makes all of them resolve to the same `session:v1:<fingerprint>`.
  Nothing errors. Claims, `check_overlap` and wait-cycle detection are all keyed
  on that identity and silently stop meaning anything: `fleet_view` shows one
  busy agent instead of three colliding ones, and a cycle needs two distinct
  nodes so it can never be seen. Observed Jul 2026 — it ran for a whole session
  before anyone noticed, and only because someone asked who owned a branch and
  the ledger could not answer. `canonical-push` now warns when one identity
  publishes a branch it did not declare while holding live claims, which is the
  only observable signature (one agent cannot publish two branches at once). It
  is a suspicion, not a verdict: switching branch with work still claimed is
  legitimate. **Mint the token per session and never write it down.**
