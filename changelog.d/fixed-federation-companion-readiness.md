- Fixed federated local planes refusing the documented companion-only setup.
  Startup now requires verified active enrollment through the protected node
  companion instead of probing a separate gRPC endpoint. Local planes need no
  remote CA or signing key. Missing companion configuration, unverified or
  rejected enrollment, and remote outages remain explicit refusals, never a
  fallback to Local arbitration.
