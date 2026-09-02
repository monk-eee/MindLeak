### Added

- Added a migration-apply gate that prevents (not just detects) an
  unreviewed migration from reaching a shared database
  (gaps.d/unaccepted-work-migration-reaches-shared-db.md). A shared or
  persistent Postgres instance is marked once via
  `ackplane-migrate --mark-shared`; every later `migrate_locked` call
  against it — from any store's `connect()`, not just the dedicated
  migration binary — refuses unless `ACKPLANE_MIGRATE_REVIEWED=1` is set,
  which should only happen after confirming the migration's key via
  `node scripts/migration-audit.mjs`. An ephemeral local or CI database is
  never marked shared and is completely unaffected.
