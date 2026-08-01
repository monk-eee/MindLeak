- Ingest is a little leaner. The error-location regexes in the execution
  extractor and the call-site regex in the AST extractor were recompiled on
  every call — once per command ingested and once per file extracted. They now
  compile once per process behind `OnceLock`, matching the Rust-structure
  extractor that already did this. Behaviour is unchanged; the zero-token write
  path just stops rebuilding constant patterns on the hot path.
