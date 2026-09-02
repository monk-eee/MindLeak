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
// A key can also be held under the *wrong content*: `migrate_locked` records
// a SHA-256 of each migration's text beside its key, so a migration that was
// applied and then edited -- exactly what ADR-0063 forbids -- refuses at
// runtime with a digest mismatch. Measured 2026-09-02: key 60 in
// `ackplane_test` held the pre-split bundled `0060_design_constitution_
// display_label.sql` (b0f93563) while committed source had since split it in
// two, so every `ConstitutionStore::connect` refused and a full test run
// reported 58 failures across five unrelated subsystems. This tool reported
// CLEAN throughout, for two independent reasons now both fixed: it compared
// keys but never content, and it only ever looked at `ackplane` -- not
// `ackplane_test`, the database ADR-0133 gave every `cargo test` run.
//
// This reports the same comparison as a repeatable command instead:
//   - two committed constants naming the same key (a static defect: no live
//     database needed to see it)
//   - a committed constant with no matching migrations/*.sql file, or vice
//     versa (also static)
//   - keys a *live* database has applied that this branch's own source never
//     declared (the actual defect above -- needs Postgres reachable;
//     skipped, not failed, when it is not)
//   - keys a live database applied under content that no longer matches the
//     committed migration now carrying that key (the rewrite above)
//   - a migration file this branch has EDITED that already exists on the
//     integration branch (static, and the author-time half of the rewrite
//     above: it is what puts a database into that state in the first place)
//
// Usage:
//   node scripts/migration-audit.mjs [--check] [--container <name>] [--base <ref>]
//   node scripts/migration-audit.mjs --next [--container <name>]
//
// --check exits 1 on a static defect (duplicate key, a constant/file
// mismatch, or an edited landed migration); the live-only findings never
// gate, since a fresh CI database can never exhibit them and a persistent
// dev container is not everyone's setup. --next prints the one key safe to
// assign, folding in the live database when reachable -- the number this
// tool exists to save you from getting wrong.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const KEY_CONST_PATTERN = /pub\(crate\) const (\w+): i64 = (-?\d+);/g;
const MIGRATION_FILE_PATTERN = /^0*(\d+)_.+\.sql$/;

/** The ref an edited-landed-migration check compares against by default. */
const DEFAULT_BASE_REF = "origin/main";

/**
 * Every database a migration key can be burned in. `ackplane` is what the
 * running services use; `ackplane_test` is what every `cargo test` run uses
 * (ADR-0133). Auditing only the first is why the key-60 rewrite above went
 * unreported: the damage was entirely in the second.
 */
const LIVE_DATABASES = ["ackplane", "ackplane_test"];

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
 * The SHA-256 `migration_lock::content_digest` records for a migration: a
 * plain hash of the SQL text, with no normalisation. Mirrored here rather
 * than approximated, because a digest computed any other way would disagree
 * with the ledger on every row and report the whole database as damaged.
 */
export function contentDigest(sql) {
  return createHash("sha256").update(Buffer.from(sql, "utf8")).digest("hex");
}

/** Committed migration text keyed by migration number, with its digest. */
export function committedDigests(migrations) {
  const byKey = new Map();
  for (const { key, name, sql } of migrations) {
    byKey.set(key, { name, digest: contentDigest(sql) });
  }
  return byKey;
}

/**
 * Every key a live database's ledger has recorded, with the content digest
 * stored beside it, or `null` when the database could not be reached. Soft
 * failure by design: this check only ever adds information, and must never
 * block on infrastructure a caller may not have running (CI never will; a
 * fresh clone may not either).
 *
 * `coalesce` renders a pre-digest-column row as an empty string rather than
 * relying on how psql prints NULL, so "no digest recorded" stays
 * distinguishable from "the empty digest".
 */
export function appliedMigrationsFromLiveDatabase(
  run,
  container,
  user,
  database,
) {
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
      "SELECT migration_key, coalesce(content_digest, '') FROM ackplane_schema_migrations ORDER BY migration_key",
    ]);
  } catch {
    return null;
  }
  return output
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const separator = line.indexOf("|");
      return {
        key: Number(line.slice(0, separator)),
        digest: line.slice(separator + 1) || null,
      };
    });
}

/**
 * Keys applied under content that is not what the committed migration now
 * carrying that key says. The runtime guard refuses these, but only once
 * someone's own connect() happens to hit the key -- surfacing as a wall of
 * failures in whatever subsystem asked first, naming a diff that is usually
 * innocent.
 *
 * A row with no digest is skipped, never reported: `migrate_locked` adopts
 * those rather than refusing, so flagging them would fire on every database
 * old enough to predate the column.
 */
export function digestMismatches(appliedRows, committedByKey) {
  return appliedRows
    .filter((row) => row.digest)
    .map((row) => ({ row, committed: committedByKey.get(row.key) }))
    .filter(
      ({ row, committed }) => committed && committed.digest !== row.digest,
    )
    .map(({ row, committed }) => ({
      key: row.key,
      name: committed.name,
      applied: row.digest,
      committed: committed.digest,
    }))
    .sort((a, b) => a.key - b.key);
}

/** Applied keys recorded before `migrate_locked` gained its digest column. */
export function legacyDigestRows(appliedRows) {
  return appliedRows
    .filter((row) => !row.digest)
    .map((row) => row.key)
    .sort((a, b) => a - b);
}

/**
 * Migration files this branch has modified that already exist on the base
 * ref, read from `git diff --name-status`.
 *
 * Only `M` counts. `A` is a brand-new migration, which is the normal way to
 * add one; `D` is a different defect with a different remedy; `R` cannot
 * apply here, since renaming a landed migration is a delete plus an add and
 * git reports the delete side.
 */
export function modifiedLandedMigrations(diffOutput) {
  return diffOutput
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => line.split(/\s+/))
    .filter(
      ([status, file]) =>
        status === "M" &&
        file &&
        MIGRATION_FILE_PATTERN.test(file.split("/").pop()),
    )
    .map(([, file]) => file)
    .sort();
}

/**
 * The same question asked of git, or `null` when the base ref is not
 * available -- a shallow clone must read as "cannot answer", never as "no
 * damage" and never as damage itself.
 *
 * Asks git for the difference rather than comparing file text here: git
 * already honours .gitattributes, so a checkout whose working copy has CRLF
 * line endings does not read as a modified migration on Windows.
 */
export function editedLandedMigrationsFromGit(run, baseRef, directory) {
  let output;
  try {
    output = run(["diff", "--name-status", baseRef, "--", directory]);
  } catch {
    return null;
  }
  return modifiedLandedMigrations(output);
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
  const baseFlag = argv.indexOf("--base");
  const baseRef =
    baseFlag !== -1 && argv[baseFlag + 1]
      ? argv[baseFlag + 1]
      : DEFAULT_BASE_REF;
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
  const migrationFilenames = fs.readdirSync(migrationsDir(repoRoot));
  const fileNumbers = committedFileNumbers(migrationFilenames);
  const committedByKey = committedDigests(
    migrationFilenames
      .map((name) => ({ name, match: MIGRATION_FILE_PATTERN.exec(name) }))
      .filter(({ match }) => match)
      .map(({ name, match }) => ({
        key: Number(match[1]),
        name,
        sql: fs.readFileSync(path.join(migrationsDir(repoRoot), name), "utf8"),
      })),
  );
  const run = (args) => execFileSync("docker", args, { encoding: "utf8" });
  const git = (args) =>
    execFileSync("git", args, { cwd: repoRoot, encoding: "utf8" });
  const live = LIVE_DATABASES.map((database) => ({
    database,
    rows: appliedMigrationsFromLiveDatabase(
      run,
      container,
      "ackplane",
      database,
    ),
  }));
  const applied = live.flatMap(({ rows }) => rows ?? []).map((row) => row.key);
  const reachable = live.some(({ rows }) => rows !== null);

  if (next) {
    console.log(nextAvailableKey(keys, fileNumbers, applied));
    if (!reachable) {
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
  for (const { database, rows } of live) {
    if (rows === null) {
      console.log(
        `  ${database}: skipped, not reachable via container "${container}"`,
      );
      continue;
    }
    const orphaned = appliedKeysNotInSource(
      rows.map((row) => row.key),
      keys,
    );
    if (orphaned.length === 0) {
      console.log(`  ${database}: none`);
      continue;
    }
    for (const key of orphaned) {
      console.log(
        `  ${database}: ${key}  <- reached this database from work nobody committed`,
      );
    }
  }

  console.log(
    "\n=== keys applied under different content than the committed migration ===",
  );
  let mismatchCount = 0;
  for (const { database, rows } of live) {
    if (rows === null) {
      console.log(
        `  ${database}: skipped, not reachable via container "${container}"`,
      );
      continue;
    }
    const mismatches = digestMismatches(rows, committedByKey);
    mismatchCount += mismatches.length;
    const legacy = legacyDigestRows(rows);
    if (mismatches.length === 0) {
      console.log(
        `  ${database}: none${legacy.length ? ` (${legacy.length} pre-digest row(s) adopted on next run, not a mismatch)` : ""}`,
      );
      continue;
    }
    for (const { key, name, applied: appliedDigest, committed } of mismatches) {
      console.log(`  ${database}: key ${key} (${name})`);
      console.log(`      applied   ${appliedDigest}`);
      console.log(`      committed ${committed}`);
      console.log(
        "      every connect() that reaches this key now refuses. The migration was edited after it applied (ADR-0063); repair the ledger row or renumber.",
      );
    }
  }

  const edited = editedLandedMigrationsFromGit(
    git,
    baseRef,
    "crates/ackplane-server/migrations",
  );
  console.log(
    `\n=== migration files this branch edited that already exist on ${baseRef} ===`,
  );
  if (edited === null) {
    console.log(
      `  skipped: ${baseRef} is not available here (a shallow clone cannot answer this)`,
    );
  } else if (edited.length === 0) {
    console.log("  none");
  } else {
    for (const file of edited) {
      console.log(`  ${file}`);
    }
    console.log(
      "      migrate_locked hashes the whole file, so editing one that has already applied -- a comment is enough -- leaves its key held under content no committed source matches, and every connect() reaching it then refuses. Restore the landed file and add a new migration instead; run --next for its key.",
    );
  }

  const editedCount = edited?.length ?? 0;
  console.log(
    `\nsummary: ${duplicates.length} duplicate key value(s), ${missingFiles.length} constant(s) with no file, ${missingKeys.length} file(s) with no constant, ${editedCount} edited landed migration(s), ${mismatchCount} key(s) applied under different content`,
  );
  if (
    check &&
    (duplicates.length > 0 ||
      missingFiles.length > 0 ||
      missingKeys.length > 0 ||
      editedCount > 0)
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
