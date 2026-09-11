- Added a runnable multi-agent path through the existing Ackplane supervisor:
  named executable configurations, separate workspaces/sessions and live task
  leases, authenticated server-compiled policy and memory packets, and durable
  worker/packet receipts applied back to Work. Active lessons and prior observed
  task outcomes inform later prompts without becoming policy or completion
  evidence. Added a real PostgreSQL/gRPC/two-process regression suite.
- Fixed cross-tenant context acceptance, cross-node supervisor updates,
  unbounded acknowledgement waits, and control-plane environment inheritance by
  child workers. Unaccounted prior runs block workspace reuse instead of silently
  launching replacement workers. Native process execution is not a sandbox;
  live agent executables and authentication remain operator prerequisites.
- Added coordinated Ctrl+C/SIGTERM shutdown with bounded receipt flushing and
  lease release, portable worker state identifiers, and declared Work scope
  carried from the Bridge/CLI through confirmation to storage.
- Verified two concurrent, authenticated Copilot CLI agents against the live
  server and Bridge in two rounds; fresh second-round prompts carried the
  previous outcomes. Added a signed, demo-only constitution publisher that
  refuses to overwrite existing policy.
