// Ackplane Compose lifecycle commands (ADR-0088 clauses 3, 7): a developer
// types `docker compose`, `podman compose`, or `node` and nothing else, and backup, restore, and
// reset are explicit operations rather than something pieced together from
// `docker compose exec` invocations remembered by hand.
//
// Platform-agnostic: node + Docker Compose or Podman Compose. Usage:
//   node scripts/ackplane-compose.mjs up                start postgres, migrate, ackplane
//   node scripts/ackplane-compose.mjs down              stop the stack, keep the volume
//   node scripts/ackplane-compose.mjs prepare ABSOLUTE_CONFIG_DIR  copy public CA and bridge salt
//   node scripts/ackplane-compose.mjs backup <file>      pg_dump the ledger to <file>
//   node scripts/ackplane-compose.mjs restore <file>     restore the ledger from <file>
//   node scripts/ackplane-compose.mjs reset --confirm    stop the stack and delete its volume

import { execFileSync } from "node:child_process";
import { X509Certificate } from "node:crypto";
import {
  constants,
  openSync,
  closeSync,
  readFileSync,
  lstatSync,
  fstatSync,
  readSync,
  writeFileSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { isAbsolute, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const POSTGRES_SERVICE = "postgres";
const POSTGRES_USER = "ackplane";
const POSTGRES_DB = "ackplane";

function composeCommand(env = process.env, probe = execFileSync) {
  const configured = env.MINDLEAK_COMPOSE_BIN;
  if (configured) {
    return configured;
  }
  for (const command of ["docker", "podman"]) {
    try {
      probe(command, ["compose", "version"], { stdio: "ignore" });
      return command;
    } catch {
      // Try the next supported compose implementation.
    }
  }
  throw new Error(
    "neither docker nor podman compose is available; set MINDLEAK_COMPOSE_BIN",
  );
}

function compose(args, options = {}) {
  execFileSync(composeCommand(), ["compose", ...args], {
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

function readPreparationFile(file, limit) {
  const before = lstatSync(file, { throwIfNoEntry: false });
  if (!before) return null;
  if (!before.isFile()) {
    throw new Error(
      `${file}: expected a regular file, not a symlink or directory`,
    );
  }
  if (before.size > limit) {
    throw new Error(`${file}: exceeds the ${limit}-byte limit`);
  }
  const descriptor = openSync(
    file,
    constants.O_RDONLY |
      (constants.O_NOFOLLOW ?? 0) |
      (constants.O_NONBLOCK ?? 0),
  );
  try {
    const opened = fstatSync(descriptor);
    if (
      !opened.isFile() ||
      opened.dev !== before.dev ||
      opened.ino !== before.ino
    ) {
      throw new Error(`${file}: regular file changed while opening`);
    }
    const buffer = Buffer.alloc(limit + 1);
    let length = 0;
    while (length < buffer.length) {
      const count = readSync(
        descriptor,
        buffer,
        length,
        buffer.length - length,
      );
      if (count === 0) break;
      length += count;
    }
    if (length > limit) {
      throw new Error(`${file}: exceeds the ${limit}-byte limit`);
    }
    if (fstatSync(descriptor).size !== length) {
      throw new Error(`${file}: regular file changed while reading`);
    }
    return buffer.subarray(0, length);
  } finally {
    closeSync(descriptor);
  }
}

function prepare(configDir, { env = process.env, run = execFileSync } = {}) {
  if (
    typeof configDir !== "string" ||
    !isAbsolute(configDir) ||
    configDir.includes("\0") ||
    (process.platform === "win32"
      ? !/^[a-z]:[\\/]/i.test(configDir)
      : configDir.startsWith("//"))
  ) {
    throw new Error(
      "prepare requires an absolute local config directory (drive-qualified on Windows)",
    );
  }
  const directory = resolve(configDir);
  const caPath = join(directory, "ackplane-dev-ca.pem");
  const saltPath = join(directory, "bridge.salt");
  const files = [
    {
      source: "ackplane:/tls/ca.crt",
      name: "ackplane-dev-ca.pem",
      target: caPath,
      limit: 65536,
    },
    {
      source: "bridge:/var/lib/ackplane-bridge/salt",
      name: "bridge.salt",
      target: saltPath,
      limit: 4096,
    },
  ];
  const checkDirectory = () => {
    const state = lstatSync(directory, { throwIfNoEntry: false });
    if (state && !state.isDirectory()) {
      throw new Error(
        `${directory}: expected a directory, not a symlink or file`,
      );
    }
  };
  checkDirectory();
  const command = composeCommand(env, run);
  const staging = mkdtempSync(join(tmpdir(), "ackplane-compose-prepare-"));
  try {
    for (const file of files) {
      try {
        run(command, ["compose", "cp", file.source, join(staging, file.name)], {
          env,
          stdio: ["ignore", "pipe", "pipe"],
        });
      } catch {
        throw new Error(`${file.target}: compose cp failed for ${file.source}`);
      }
    }
    for (const file of files) {
      file.bytes = readPreparationFile(join(staging, file.name), file.limit);
      if (!file.bytes?.length) {
        throw new Error(
          `${file.target}: staged copy must be a nonempty regular file`,
        );
      }
    }
    try {
      if (!new X509Certificate(files[0].bytes).ca) throw new Error();
    } catch {
      throw new Error(`${caPath}: staged copy is not a valid CA certificate`);
    }
    checkDirectory();
    const matchesExisting = (file) => {
      const existing = readPreparationFile(file.target, file.limit);
      if (existing === null) return false;
      if (!existing.equals(file.bytes)) {
        throw new Error(
          `${file.target}: existing data differs; refusing to overwrite`,
        );
      }
      return true;
    };
    const missing = files.filter((file) => !matchesExisting(file));
    if (missing.length > 0) {
      mkdirSync(directory, { recursive: true, mode: 0o700 });
      checkDirectory();
      for (const file of missing) {
        let descriptor;
        try {
          descriptor = openSync(file.target, "wx", 0o600);
        } catch (error) {
          if (error.code === "EEXIST" && matchesExisting(file)) continue;
          throw new Error(
            `${file.target}: exclusive creation failed (${error.code ?? "I/O error"})`,
          );
        }
        try {
          writeFileSync(descriptor, file.bytes);
        } finally {
          closeSync(descriptor);
        }
      }
    }
    return { caPath, saltPath };
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
}

function backup(file) {
  if (!file) {
    throw new Error("usage: node scripts/ackplane-compose.mjs backup <file>");
  }
  const out = openSync(file, "w");
  try {
    execFileSync(
      composeCommand(),
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
    composeCommand(),
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
    case "prepare": {
      if (rest.length !== 1) {
        throw new Error(
          "usage: node scripts/ackplane-compose.mjs prepare ABSOLUTE_CONFIG_DIR",
        );
      }
      const { caPath, saltPath } = prepare(rest[0]);
      console.log(`ackplane-compose: prepared ${caPath} and ${saltPath}`);
      return;
    }
    case "backup":
      return backup(rest[0]);
    case "restore":
      return restore(rest[0]);
    case "reset":
      return reset(rest.includes("--confirm"));
    default:
      throw new Error(
        "usage: node scripts/ackplane-compose.mjs <up|down|prepare|backup|restore|reset> [args]",
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

export { composeCommand, up, down, prepare, backup, restore, reset };
