- Local MCP builds now ignore inherited Git repository pointers when recording
  their source revision and Cargo watch paths. Direct Cargo invocations cannot
  accidentally identify another checkout; explicit build-SHA overrides retain
  their existing precedence and normalization.
