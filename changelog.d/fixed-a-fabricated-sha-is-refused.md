- **A commit sha that git cannot resolve is now refused instead of recorded.**
  `ingest_commit` accepted whatever string it was handed and made
  `intent:<that string>` the node id. Two failures followed from that, and both
  happened. An abbreviation like `7b17243` recorded a *second* node for a commit
  that may already have one, splitting a single commit's provenance across two
  nodes with half the edges each. Worse, a *fabricated* sha — an agent holding
  the abbreviation and composing the remaining thirty-three characters — created
  a phantom intent node carrying real edges to real files, indistinguishable
  afterwards from a genuine record, and permanent: there is no verb that removes
  a node.
  Ingestion now requires a full forty-hex-digit object name and, when the
  checkout is known, resolves it with `git cat-file -e <sha>^{commit}`. Shape
  alone would not have been enough — the invention that prompted this was a
  perfectly well-formed forty-character hex string, and only git can tell a
  plausible object name from a real one. Both refusals name the command that
  produces a real value (`git rev-parse`), because the caller is the only one who
  can stop supplying a wrong one. Checking is deterministic and token-free, so
  the ingest path stays on the zero-token side of invariant 1.
  Tests that used labels like `proof123` or `session-a` now build real object
  names, including one that reproduces the fabrication against a temporary
  repository: same length, same alphabet, same leading characters, and refused.
