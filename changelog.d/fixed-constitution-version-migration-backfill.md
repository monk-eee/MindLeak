### Fixed
- The constitution-version migration no longer re-stamps every goal whose
  `constitution_version` is `NULL` to the long-superseded literal
  `'constitution:v1'` on every database open. The one-time freeze (existing
  pre-versioning goals become v1) now only fires once, guarded the same way
  the version-creation insert already is; a plain `Objective` created via
  `define_goal` after any version already exists keeps its `NULL`
  `constitution_version` across a server restart instead of being silently
  mislabeled. A narrow, named repair also clears the wrong tag from the three
  live goals it had already mislabeled.
