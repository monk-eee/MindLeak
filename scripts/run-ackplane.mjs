// Run an Ackplane binary (bridge or server) with .env loaded into its
// environment, the same way `docker compose` reads .env for the containers
// in docker-compose.yml -- so a developer types one command and nothing
// else, instead of setting a handful of $env:/export vars by hand each time
// (the earlier ad-hoc approach is how a Bridge salt file gets started fresh,
// undocumented, and then lost when the terminal closes).
//
// Platform-agnostic: node only. Usage:
//   node scripts/run-ackplane.mjs bridge   runs target/release/ackplane-bridge
//   node scripts/run-ackplane.mjs server   runs target/release/ackplane-server
//
// Reads .env from the repo root (see .env.example for the documented
// defaults); real env vars already set in the calling shell always win over
// .env, matching standard dotenv precedence.

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const BINARIES = {
  bridge: "ackplane-bridge",
  server: "ackplane-server",
};

/** Parse simple KEY=VALUE lines; blank lines and #-comments are skipped. */
export function parseEnvFile(text) {
  const values = {};
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;
    const eq = line.indexOf("=");
    if (eq === -1) continue;
    values[line.slice(0, eq).trim()] = line.slice(eq + 1).trim();
  }
  return values;
}

/** .env values merged under whatever the calling shell already set. */
export function resolveEnv(envFilePath, currentEnv) {
  const fromFile = existsSync(envFilePath)
    ? parseEnvFile(readFileSync(envFilePath, "utf8"))
    : {};
  return { ...fromFile, ...currentEnv };
}

function binaryPath(name) {
  const suffix = process.platform === "win32" ? ".exe" : "";
  return `target/release/${name}${suffix}`;
}

function run(binaryKey) {
  const binaryName = BINARIES[binaryKey];
  if (!binaryName) {
    throw new Error(
      `unknown binary '${binaryKey}': expected 'bridge' or 'server'`,
    );
  }
  const path = binaryPath(binaryName);
  if (!existsSync(path)) {
    throw new Error(
      `${path} does not exist -- build it first: cargo build --release -p ackplane-server`,
    );
  }
  const env = resolveEnv(".env", process.env);
  const result = spawnSync(path, [], { stdio: "inherit", env });
  if (result.error) throw result.error;
  process.exitCode = result.status ?? 1;
}

function main(argv) {
  const [command] = argv;
  if (!command) {
    throw new Error("usage: node scripts/run-ackplane.mjs <bridge|server>");
  }
  run(command);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(`run-ackplane: ${error.message}`);
    process.exitCode = 1;
  }
}
