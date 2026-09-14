- **Failed Bridge startup now reports a failing process exit:** configuration,
  salt-file, database initialization, listener, and serving errors retain their
  diagnostics and exit with status 1 instead of appearing successful to scripts
  and service managers. Loopback restrictions and response behavior are unchanged.
