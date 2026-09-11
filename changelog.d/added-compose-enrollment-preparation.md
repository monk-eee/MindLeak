- **`ackplane-compose prepare ABSOLUTE_CONFIG_DIR` prepares local enrollment
  trust without replacing it.** It stages only the public CA and Bridge tenant
  salt, validates bounded regular files and the CA certificate, and creates
  missing files exclusively with restrictive permissions. Identical reruns keep
  existing bytes and timestamps; changed trust, symlinks, invalid copies, and
  ambiguous destinations are refused. No private signing or server key is
  exported, and preparation never enrolls or approves a node. A late filesystem
  error can leave a newly created first file; a rerun safely fills its missing
  partner after the error is resolved.
