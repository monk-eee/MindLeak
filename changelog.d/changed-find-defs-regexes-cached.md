- The last per-call regex recompilation on the ingest path is gone. The AST
  extractor's `find_defs` recompiled each language's definition patterns on every
  file it processed; they are now compiled once and cached by pattern text,
  completing the hoisting begun for the execution and call-site extractors.
  Behaviour is unchanged.
