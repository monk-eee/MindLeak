- **Symbol and import extraction remains heuristic and partially scoped.** —
  Static JS/TS named imports now produce cross-file `calls`, but default and
  namespace calls, re-exports, path aliases, dynamic imports, and other language
  import syntaxes are not resolved. Type hierarchy supports simple named local
  and imported JS/TS heritage, not default/namespace targets or expression-based
  mixins. Non-JS brace/indent extractors also remain regex-based. — Medium impact
  on graph completeness. — Tracked: expand fixture-backed deterministic parsers;
  Tree-sitter remains the precision upgrade (ADR-0002).
