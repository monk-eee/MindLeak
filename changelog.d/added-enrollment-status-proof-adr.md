### Added

- Added proposed ADR-0122, defining a privacy-preserving enrollment-status
  proof for `NodeEnrollmentService`: a candidate node signs a
  fingerprint-scoped, domain-separated proof of possession before learning
  its binding's lifecycle state, and every verification failure (absent
  binding, wrong key, bad signature, stale timestamp, replayed nonce)
  collapses to one indistinguishable "not enrolled" result so a caller can
  never enumerate another repository's or node's state. Closes the design
  gap in `gaps.d/ackplane-client-cannot-detect-unenrolled-repositories.md`.
