- **The Industrial quickstart now follows the enrolled companion contract.**
  Enrollment explicitly selects the credential-facility provider and one
  persistent user-local state directory, keeps administrator approval separate,
  and starts `register-me serve` before consumers. The supervisor and MCP
  examples use companion directory/tenant/repository settings instead of obsolete
  node/key/seed overrides. Executable examples are contract-tested; the portable
  launcher now supports `register-me` and `supervisor`, preserves arguments,
  selects Windows executables, and reports child failure without echoing secrets.
