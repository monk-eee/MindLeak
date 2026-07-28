- **Symbols now embed their declaration and doc comment, so `recall` finds code
  instead of the tests that exercise it (ADR-0008).** A symbol node stored
  `path:line` as its entire content, so the only thing an embedding could see
  was the symbol's *name*. Terse implementation names (`effective_weight`,
  `prune`, `recall`) embedded as near-noise, while long descriptive test names
  embedded richly — so recall systematically returned the tests instead of the
  code under test. Measured: asking *"how is effective edge weight computed from
  half-life decay"* returned six test functions and not `effective_weight`, and
  querying the literal identifier `effective_weight` did not return
  `effective_weight` either. After the change the same question returns
  `decay.rs:effective_weight` at **0.801**, top hit, with its doc comment
  included in the result — the answer arrives without opening the file.
  Extraction stays deterministic and zero-token: the declaration line and any
  doc comment directly above it are text already parsed off disk, never a model
  call. Bounded to 8 comment lines and 400 characters so a licence header never
  becomes a symbol's meaning, and Rust attributes between a doc comment and its
  declaration are stepped over rather than treated as the end of the comment.
