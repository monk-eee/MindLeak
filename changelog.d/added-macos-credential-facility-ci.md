### Added

- Added a strict macOS Keychain credential-facility CI job. It requires the real `CredentialFacilitySigner` round trip to execute rather than allowing an unavailable backend to report a skipped test as success.
