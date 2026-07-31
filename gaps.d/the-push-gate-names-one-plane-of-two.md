- **The push gate names one plane of two, so a publish can outrun its evidence —
  MEASURED 2026-07-31, OPEN.**
  — *What happened.* `node scripts/canonical-push.mjs` from a fresh linked
  worktree published the branch and then reported, on the same run:
  `canonical-push: published HEAD -> origin/<branch>` immediately followed by
  `canonical-push: published commit not recorded; the Memory Plane was
  unreachable, so this work will not certify`. The push had already happened.
  No `target/completion-offers/task-<id>.json` was written, so the documented
  "submit the completion offer immediately" path was simply unavailable, and the
  commit was absent from the graph until it was ingested by hand.
  — *Where, and why it is not just an unset variable.* `resolveServer` in
  [`scripts/claim-gate.mjs`](../scripts/claim-gate.mjs) resolves each plane from
  a per-plane override (`serverBinaries`: `LODESTAR_MCP_BIN` for lodestar,
  `MINDLEAK_MCP_BIN` for mindleak) and otherwise from
  `<repoRoot>/target/{release,debug}`. A fresh worktree has no build, so **both**
  planes are unresolved unless both overrides are set. Its own doc comment states
  the intent exactly: both planes resolve the same way deliberately, because "a
  caller that can reach the ledger but not the graph would record intent and drop
  the evidence for it, which is the exact asymmetry that leaves published work
  uncertifiable". The symmetry holds in the code and is then defeated by what the
  repository tells the operator: `LODESTAR_MCP_BIN` is named in `DEVELOPERS.md`
  and, inside the gate itself, in two operator-facing failure messages and a doc
  comment; `MINDLEAK_MCP_BIN` appears nowhere but the `serverBinaries` table —
  in **no living document, and in no message the operator will ever be shown**.
  Its only prose mention anywhere is ADR-0073, which is a historical record.
  An agent that does what the guidance and the error text say sets one override,
  and lands in precisely the asymmetry the comment was written to prevent.
  — *Impact.* The failure is ordered the wrong way round to be safe: the branch
  is already published when the warning appears, and the warning is advisory, so
  a run that has irreversibly pushed reports its own uncertifiability and exits.
  Recovery is manual — ingest the commit, then drive
  `evidence_for` (Memory Plane) → `check_conformance` → `task_transition` by hand
  — and an agent that reads "published" and stops has shipped work that can never
  certify. Observed once on PR #307; the cost was recoverable but only because
  the message was read closely.
  — *Not fixed this run.* Two directions, neither taken here because the choice
  is a design decision rather than a typo: name both overrides wherever one is
  named today (the gate's failure text and `DEVELOPERS.md`), or make the gate
  resolve and check both planes *before* it pushes, so an unreachable Memory
  Plane refuses the publish instead of annotating it afterwards. The second is
  the stronger reading of the `resolveServer` comment, since a warning issued
  after an irreversible act is not the same protection as a refusal before it.
