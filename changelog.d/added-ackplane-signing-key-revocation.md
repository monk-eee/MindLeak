- **Added:** `ackplane-server`'s signing-key registry can now revoke a key
  (`signing_keys::revoke`), ending its authority immediately and idempotently
  (a second revocation is a no-op that never moves `revoked_at` forward). This
  completes ADR-0085 decision 8: the stream-termination consequence for a
  revoked key already existed, but nothing could previously trigger it.
