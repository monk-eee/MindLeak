#!/usr/bin/env node
// Audit ackplane-server's Postgres migration-key ledger.
//
// `migrate_locked` (crates/ackplane-server/src/migration_lock.rs) serialises
// schema migrations under an advisory lock keyed by a bare integer, tracked
// in a shared table (`ackplane_schema_migrations`) that persists across
// however many concurrent branches' local dev testing touches it. Two
// branches independently computing "the next available key" from their own
// view of committed main can pick the same number; whichever's SQL reaches
// the shared database first wins the key, and the second branch's
// `migrate_locked` call silently no-ops against a schema it never wrote
// (gaps.d/unaccepted-work-migration-reaches-shared-db.md). Confirmed twice
// in one week (keys 19 and 27) before this tool existed, always discovered
// by hand via ad hoc `git grep` plus a manual `psql` query.
//
// This reports the same comparison as a repeatable command instead:
//   - two committed constants naming the same key (a static defect: no live
//     database needed to see it)
//   - a committed constant with no matching migrations/*.sql file, or vice
//     versa (also static)
//   - keys the *live* shared database has applied that this branch's own
//     source never declared (the actual defect above -- needs Postgres
//     reachable; skipped, not failed, when it is not)
//
// Usage:
//   node scripts/migration-audit.mjs [--check] [--container <name>]
//   node scripts/migration-audit.mjs --next [--container <name>]
//
// --check exits 1 on a static defect (duplicate key, or a constant/file
// mismatch); the live-only finding never gates, since a fresh CI database
// can never exhibit it and a persistent dev container is not everyone's
// setup. --next prints the one key safe to assign, folding in the live
// database when reachable -- the number this tool exists to save you from
// getting wrong.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const KEY_CONST_PATTERN = /pub\(crate\) const (\w+): i64 = (-?\d+);/g;
const MIGRATION_FILE_PATTERN = /^0*(\d+)_.+\.sql$/;

/**
 * Every named migration key declared in migration_lock.rs's `key` module.
 * `GLOBAL_SCHEMA` is excluded: it names the lock namespace every migration
 * takes first, not a migration of its own, so it never has a matching file.
 */
export function committedKeys(source) {
  const found = [];
  for (const match of source.matchAll(KEY_CONST_PATTERN)) {
    const [, name, value] = match;
    if (name === "GLOBAL_SCHEMA") continue;
    found.push({ name, key: Number(value) });
  }
  return found;
}

/** The migration number each committed `migrations/*.sql` filename declares. */
export function committedFileNumbers(filenames) {
  return filenames
    .map((name) => MIGRATION_FILE_PATTERN.exec(name))
    .filter(Boolean)
    .map((match) => Number(match[1]))
    .sort((a, b) => a - b);
}

/**
 * Two constants naming the same key collide the moment either runs:
 * `migrate_locked` treats "this key is applied" as one fact, so whichever
 * constant's SQL text reaches a fresh database first silently wins, and the
 * other's migration never actually runs anywhere.
 */
export function duplicateKeyValues(keys) {
  const byValue = new Map();
  for (const { name, key } of keys) {
    if (!byValue.has(key)) byValue.set(key, []);
    byValue.get(key).push(name);
  }
  return [...byValue.entries()]
    .filter(([, names]) => names.length > 1)
    .map(([key, names]) => ({ key, names }))
    .sort((a, b) => a.key - b.key);
}

/** Committed constants with no matching migrations/*.sql file: a key the code claims but nothing on disk actually runs. */
export function keysMissingFiles(keys, fileNumbers) {
  const files = new Set(fileNumbers);
  return keys
    .filter(({ key }) => !files.has(key))
    .sort((a, b) => a.key - b.key);
}

/** Committed migration files with no matching key constant: SQL nothing calls `migrate_locked` with, so it can never actually apply. */
export function filesMissingKeys(keys, fileNumbers) {
  const declared = new Set(keys.map((entry) => entry.key));
  return fileNumbers.filter((number) => !declared.has(number));
}

/**
 * Keys the live ledger marks applied that this branch's own source never
 * declared -- the exact defect gaps.d/unaccepted-work-migration-reaches-
 * shared-db.md describes: a key that reached a shared database from work
 * nobody committed.
 */
export function appliedKeysNotInSource(appliedKeys, keys) {
  const declared = new Set(keys.map((entry) => entry.key));
  return appliedKeys.filter((key) => !declared.has(key)).sort((a, b) => a - b);
}

/**
 * The one key safe to assign next, folding in every source of truth this
 * repo has: committed constants, committed files, and -- when reachable --
 * whatever the live shared database already has applied. Picking "next"
 * from committed main alone is exactly how two concurrent branches have
 * twice picked the same number in one week.
 */
export function nextAvailableKey(keys, fileNumbers, appliedKeys = []) {
  const known = [
    0,
    ...keys.map((entry) => entry.key),
    ...fileNumbers,
    ...appliedKeys,
  ];
  return Math.max(...known) + 1;
}

/**
 * The keys a live database's own ledger has recorded as applied, or `null`
 * when none could be reached. Soft failure by design: this check only ever
 * adds information, and must never block on infrastructure a caller may not
 * have running (CI never will; a fresh clone may not either).
 */
export function appliedKeysFromLiveDatabase(run, container, user, database) {
  let output;
  try {
    output = run([
      "exec",
      container,
      "psql",
      "-U",
      user,
      "-d",
      database,
      "-t",
      "-A",
      "-c",
      "SELECT migration_key FROM ackplane_schema_migrations ORDER BY migration_key",
    ]);
  } catch {
    return null;
  }
  return output
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map(Number);
}

function migrationsDir(repoRoot) {
  return path.join(repoRoot, "crates", "ackplane-server", "migrations");
}

function lockFilePath(repoRoot) {
  return path.join(
    repoRoot,
    "crates",
    "ackplane-server",
    "src",
    "migration_lock.rs",
  );
}

async function main() {
  const argv = process.argv.slice(2);
  const check = argv.includes("--check");
  const next = argv.includes("--next");
  const containerFlag = argv.indexOf("--container");
  const container =
    containerFlag !== -1 && argv[containerFlag + 1]
      ? argv[containerFlag + 1]
      : (process.env.ACKPLANE_POSTGRES_CONTAINER ?? "ackplane-postgres-1");

  const repoRoot = path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    "..",
  );
  const source = fs.readFileSync(lockFilePath(repoRoot), "utf8");
  const keys = committedKeys(source);
  const fileNumbers = committedFileNumbers(
    fs.readdirSync(migrationsDir(repoRoot)),
  );
  const run = (args) => execFileSync("docker", args, { encoding: "utf8" });
  const applied = appliedKeysFromLiveDatabase(
    run,
    container,
    "ackplane",
    "ackplane",
  );

  if (next) {
    console.log(nextAvailableKey(keys, fileNumbers, applied ?? []));
    if (applied === null) {
      console.error(
        `(no live database reachable via container "${container}"; based on committed source only)`,
      );
    }
    return;
  }

  const duplicates = duplicateKeyValues(keys);
  const missingFiles = keysMissingFiles(keys, fileNumbers);
  const missingKeys = filesMissingKeys(keys, fileNumbers);

  console.log(
    "=== duplicate key values (two constants naming the same migration) ===",
  );
  if (duplicates.length === 0) console.log("  none");
  for (const { key, names } of duplicates)
    console.log(`  key ${key}: ${names.join(", ")}`);

  console.log(
    "\n=== committed constants with no matching migrations/*.sql file ===",
  );
  if (missingFiles.length === 0) console.log("  none");
  for (const { name, key } of missingFiles) console.log(`  ${name} = ${key}`);

  console.log(
    "\n=== committed migrations/*.sql files with no matching constant ===",
  );
  if (missingKeys.length === 0) console.log("  none");
  for (const key of missingKeys) console.log(`  ${key}`);

  console.log(
    "\n=== keys the live database has applied but this branch's source never declared ===",
  );
  if (applied === null) {
    console.log(
      `  skipped: no live database reachable via container "${container}"`,
    );
  } else {
    const orphaned = appliedKeysNotInSource(applied, keys);
    if (orphaned.length === 0) console.log("  none");
    for (const key of orphaned) {
      console.log(
        `  ${key}  <- reached the shared database from work nobody committed`,
      );
    }
  }

  console.log(
    `\nsummary: ${duplicates.length} duplicate key value(s), ${missingFiles.length} constant(s) with no file, ${missingKeys.length} file(s) with no constant`,
  );
  if (
    check &&
    (duplicates.length > 0 || missingFiles.length > 0 || missingKeys.length > 0)
  ) {
    process.exitCode = 1;
  }
}

const invoked =
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invoked) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 2;
  });
}
