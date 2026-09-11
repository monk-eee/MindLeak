- Added `register-me serve`, the long-lived enrolled identity owner. Supervisor,
  MCP status/work reads and federated claims use protected, bounded local IPC
  without loading or copying private keys. Recovery preserves the original
  provider; credential loss or authority refusal closes streams and stops owned
  workers while retaining unconfirmed evidence. Legacy runtime seed/key overrides
  are refused. Native enrollment/restart, concurrent worker and MCP subprocess
  tests cover the handoff; clean-install and production-authentication gates
  remain separate requirements.
