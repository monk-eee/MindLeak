- **Fixed:** `acquire_migration_lock` (`mindleak-storage`) now also retries a
  `PermissionDenied` from `create_new` on Windows, not only `AlreadyExists`.
  A concurrent holder's `remove_file` (its own `Drop`) does not clear the
  directory entry atomically the way Unix `unlink` does — NTFS marks the
  file for pending delete first, during which a racing `create_new` can
  observe `PermissionDenied` instead of the clean `AlreadyExists`/success the
  retry loop already handled. This flaked
  `repository::tests::migration_lock_retries_until_a_concurrent_holder_releases_it`
  in a full workspace `cargo test --all` run while the isolated test passed
  every time — the signature of a timing race, confirmed with a red/green
  proof: a new stress test repeating the exact race 50 times failed 3 of 5
  runs without the fix and passed 8 of 8 with it. Retrying is bounded by the
  same attempt loop as the existing `AlreadyExists` arm, so a genuinely
  unwritable directory still surfaces as `MigrationBusy` rather than
  hanging, and the extra retry is scoped to Windows (`#[cfg(windows)]`) since
  the race it exists for cannot occur on Unix.
