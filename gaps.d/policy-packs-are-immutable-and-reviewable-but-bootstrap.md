- **Policy packs are immutable and reviewable, but bootstrap activation and
  upgrade amendment diffs are not yet implemented.** — ADR-0026 task 2 now
  validates/registers immutable pack versions, proposes Common Core, persists
  dispositions, materializes adopted clauses with source provenance, and blocks
  conflicts/upstream rewrites. An ungoverned project still needs task 3's
  deterministic fact discovery plus atomic draft activation; a newer pack
  version is deliberately refused until task 5 supplies an attributed amendment
  diff. — Medium onboarding impact, no silent policy risk: current behavior
  fails closed (`draft` / `needs_human` / amendment-required) rather than
  inheriting or auto-activating policy.
