- **Rust impact traversal crosses nested module and verified crate boundaries.**
  Nested grouped `use` trees now retain every module branch and alias. Imports
  through local Cargo path dependencies resolve to real workspace artifacts only
  when the nearest consumer manifest declares that dependency and its target
  manifest declares the crate root; unresolved, external, and ambiguous imports
  keep the conservative `package:<name>` fallback. Re-ingest processes manifests
  before source files so deferred artifact candidates converge regardless of
  lexical crate order.
