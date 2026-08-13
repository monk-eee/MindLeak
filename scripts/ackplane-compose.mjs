// Ackplane Compose lifecycle commands (ADR-0088 clauses 3, 7): a developer
// types `docker compose` or `node` and nothing else, and backup, restore, and
// reset are explicit operations rather than something pieced together from
// `docker compose exec` invocations remembered by hand.
//
// Platform-agnostic: node + docker compose only. Usage:
//   node scripts/ackplane-compose.mjs up                start postgres, migrate, ackplane
//   node scripts/ackplane-compose.mjs down              stop the stack, keep the volume
//   node scripts/ackplane-compose.mjs backup <file>      pg_dump the ledger to <file>
//   node scripts/ackplane-compose.mjs restore <file>     restore the ledger from <file>
//   node scripts/ackplane-compose.mjs reset --confirm    stop the stack and delete its volume

import { execFileSync } from "node:child_process";
import { openSync, closeSync, readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const POSTGRES_SERVICE = "postgres";
const POSTGRES_USER = "ackplane";
const POSTGRES_DB = "ackplane";

function compose(args, options = {}) {
  execFileSync("docker", ["compose", ...args], {
    stdio: "inherit",
    ...options,
  });
}

function up() {
  compose(["up", "-d", "--build"]);
}

function down() {
  compose(["down"]);
}

function backup(file) {
  if (!file) {
    throw new Error("usage: node scripts/ackplane-compose.mjs backup <file>");
  }
  const out = openSync(file, "w");
  try {
    execFileSync(
      "docker",
      [
        "compose",
        "exec",
        "-T",
        POSTGRES_SERVICE,
        "pg_dump",
        "-U",
        POSTGRES_USER,
        POSTGRES_DB,
      ],
      { stdio: ["ignore", out, "inherit"] },
    );
  } finally {
    closeSync(out);
  }
  console.log(`ackplane-compose: backed up ${POSTGRES_DB} to ${file}`);
}

function restore(file) {
  if (!file) {
    throw new Error("usage: node scripts/ackplane-compose.mjs restore <file>");
  }
  const dump = readFileSync(file);
  execFileSync(
    "docker",
    [
      "compose",
      "exec",
      "-T",
      POSTGRES_SERVICE,
      "psql",
      "-U",
      POSTGRES_USER,
      "-d",
      POSTGRES_DB,
    ],
    { input: dump, stdio: ["pipe", "inherit", "inherit"] },
  );
  console.log(`ackplane-compose: restored ${POSTGRES_DB} from ${file}`);
}

// Reset requires an unambiguous confirmation (ADR-0088 clause 7): the ledger
// is real data, and "docker compose down -v" is one autocomplete away from
// "docker compose down" for anyone typing it by hand. This command exists so
// deleting the volume is a decision made in this script's argument, not a
// keystroke slip in a longer one.
function reset(confirmed) {
  if (!confirmed) {
    throw new Error(
      "reset deletes the ackplane-postgres-data volume and everything in it. " +
        "Re-run as: node scripts/ackplane-compose.mjs reset --confirm",
    );
  }
  compose(["down", "--volumes"]);
  console.log("ackplane-compose: stack stopped and its volume removed");
}

function main(argv) {
  const [command, ...rest] = argv;
  switch (command) {
    case "up":
      return up();
    case "down":
      return down();
    case "backup":
      return backup(rest[0]);
    case "restore":
      return restore(rest[0]);
    case "reset":
      return reset(rest.includes("--confirm"));
    default:
      throw new Error(
        "usage: node scripts/ackplane-compose.mjs <up|down|backup|restore|reset> [args]",
      );
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(`ackplane-compose: ${error.message}`);
    process.exitCode = 1;
  }
}

export { up, down, backup, restore, reset };
