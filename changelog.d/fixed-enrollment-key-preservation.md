- `register-me` no longer replaces an unreadable or malformed enrollment key,
  or generates a new key during activation. Missing keys with saved enrollment
  state require recovery of the original key. New keys are published atomically
  without overwriting another process's identity and are owner-only on Unix;
  activation rejects a key that differs from the saved enrollment fingerprint.
