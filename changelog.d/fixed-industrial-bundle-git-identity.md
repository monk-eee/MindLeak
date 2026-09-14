- Industrial host archive source checks now ignore inherited Git repository
  pointers, so the recorded revision and clean-source guard describe the
  requested checkout. The same isolated environment reaches Cargo and archive
  subprocesses, preventing build scripts from embedding another repository's
  revision. Startup checks also verify the source revisions reported by the
  Local MCP binaries. Failed Git reads or mismatched revisions refuse packaging
  before an archive is published.
