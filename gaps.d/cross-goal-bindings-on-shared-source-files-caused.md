- **Cross-goal bindings on shared *source* files caused false drift — RESOLVED.** —
  Repeated per-task `link_goal_to_code` calls left 10 lodestar /
  mindleak source files each bound to two active goals (e.g. `model.rs`, `lib.rs`,
  `store/coordination.rs`, `facade/conformance.rs`,
  `crates/mindleak-core/src/graph/evidence.rs`), so a commit serving goal A reports
  drift against goal B. — RESOLVED for documentation: goals govern code, not the
  shared prose every task touches, so `evaluate_conformance` now ignores `governed`
  bindings on documentation nodes **at read time** — deleting nothing (commit
  `8ce8516`, which superseded and removed the rejected auto-delete-on-restart
  *clobber* `b55f2a0`; an explicit `forbid_change` lock on a doc is still honoured).
  The one-time clobber had already dropped the 10 documentation bindings (89 → 79)
  before removal; those were benign pollution and are re-linkable. `unlink_goal_from_code`
  + `governing_goals` (commit `6b22bca`) provide an explicit, audited prune path. —
  **RESOLVED Jul 2026 (task:c4bae4cc6ec2)** via human-in-the-loop
  `unlink_goal_from_code` triage: each file's true owner is its plane's objective,
  so the mistaken bindings were the *MindLeak-graph* objective
  (`local-temporal-context-graph`) on the 8 Lodestar source files, and the
  `principled-verified-delivery` **constraint** (a cross-cutting rule, not a
  per-file owner) on `model.rs` and `graph/evidence.rs`. Those 10 bindings were
  dropped (explicit/audited, no auto-delete); each of the 10 files now has exactly
  one governing goal, so honest commits no longer drift. Data-plane only — no code
  change. — **Follow-up Jul 2026:** the triage did not, and could not, cover files
  whose two bindings are *both* accurate — `crates/mindleak-mcp/src/tools/mod.rs`
  is legitimately the graph engine's MCP surface *and* the ADR-0030 session
  registrar. That residue is addressed by ADR-0041 (declared coverage), not by
  more unlinking: there is no wrong binding left to remove.
