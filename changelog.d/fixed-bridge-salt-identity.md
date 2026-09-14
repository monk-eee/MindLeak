- **Bridge salt loading preserves tenant identity:** empty salts and read errors
  now stop startup without replacing existing bytes. First-time creation
  publishes a complete salt without overwriting another creator's file, and
  concurrent callers reuse the winning salt. New salt files have owner-only
  permissions on Unix. Existing nonempty salts remain unchanged; restore the
  original salt or correct its permissions after a load failure rather than
  deleting it and selecting a different tenant identity.
