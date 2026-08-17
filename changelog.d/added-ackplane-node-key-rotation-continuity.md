- Ackplane's `NodeEnrollmentService.RotateNodeKey` now applies rotations
  instead of refusing every call as unimplemented (ADR-0085 decision 7). A node
  proves continuity by signing the identical rotation statement with both its
  current key and the successor key it already holds; Ackplane verifies both
  signatures, registers the successor, and retires the current key at a
  bounded overlap so records already signed by it keep resolving. A current
  key that is unknown, not yet active, expired, retired, or revoked is
  rejected rather than treated as authorising anything, and a successor key id
  that already exists cannot be reused.
