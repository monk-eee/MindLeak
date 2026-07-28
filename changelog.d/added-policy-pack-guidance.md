- **Policy pack authoring and upgrading is documented (SPEC-CONSTITUTION §6,
  §12.1 task 6).** `docs/POLICY-PACKS.md` covers the pack and clause schema,
  namespacing and why a clause without a scope, evidence contract and
  consequence is deliberately review-only, the adoption sequence, and the
  upgrade path. The organising rule is stated first because everything else
  follows from it: composition happens at proposal time, never through live
  inheritance, so an adopted clause is copied into the project with its source
  pack id, version, digest and key, and re-publishing upstream cannot change
  local law. The guide ends by naming the tests that enforce each limit rather
  than asserting good behaviour, so a reader can check the claims instead of
  trusting them.
