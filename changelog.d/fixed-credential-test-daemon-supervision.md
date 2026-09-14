- Linux credential-test sessions now supervise their foreground keyring daemon
  through startup, test execution and shutdown. Losing the daemon fails promptly
  and terminates only the owned test process group instead of leaving credential
  calls blocked until the CI job timeout. Startup and cleanup are bounded, test
  failures and interruption statuses are preserved, and daemon output is never
  evaluated or echoed.
