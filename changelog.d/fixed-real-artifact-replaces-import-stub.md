- **A real source file can replace the import stub created before it.** When
  importers were ingested first, their unresolved Rust candidates could leave the
  eventual module root behind an alias. The real file then promoted that alias
  before writing its own structural edges, so an edge still targeting the deleted
  alias failed the transaction with a SQLite foreign-key error. Alias promotion
  now runs after the real file's edges are inserted, allowing the same transaction
  to retarget them safely. A fresh repository-wide re-ingest now processes all
  280 extractable tracked files with zero failures; the previous ordering failed
  on four `lib.rs` / `mod.rs` roots.
