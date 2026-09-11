- `register-me` now explicitly selects `credential-facility-software` and an
  absolute state directory instead of creating or loading raw seed files. Public
  requests are saved before transmission and replayed without changing identity;
  activation and lost-response recovery use the node provider, and the enrollment
  event is stable across retries. Legacy key options are refused. Added native
  CLI and real-service retry coverage, with mandatory isolated Linux credential
  sessions in industrial/coverage tests and native Windows/macOS restart checks.
  Supervisor and MCP status-loader adoption remain unfinished.
