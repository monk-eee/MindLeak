// Run an Ackplane binary with .env loaded into its
// environment, the same way `docker compose` reads .env for the containers
// in docker-compose.yml -- so a developer types one command and nothing
// else, instead of setting a handful of $env:/export vars by hand each time
// (the earlier ad-hoc approach is how a Bridge salt file gets started fresh,
// undocumented, and then lost when the terminal closes).
//
// Platform-agnostic: node only. Usage:
//   node scripts/run-ackplane.mjs <bridge|server|register-me|supervisor> [args...]
//
// Run from the workspace root: .env and target/release resolve against the
// calling shell's cwd, which the child retains even when this script is invoked
// by absolute path. Real env vars already set in the shell always win over .env.
// Arguments are passed unchanged; enrollment and serve commands remain explicit.

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const BINARIES = {
  bridge: "ackplane-bridge",
  server: "ackplane-server",
  "register-me": "register-me",
  supervisor: "ackplane-supervisor",
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

function binaryPath(name, platform, cwd) {
  const suffix = platform === "win32" ? ".exe" : "";
  return resolve(cwd, "target", "release", `${name}${suffix}`);
}

export function run(
  binaryKey,
  args = [],
  {
    cwd = process.cwd(),
    env = process.env,
    platform = process.platform,
    exists = existsSync,
    spawn = spawnSync,
  } = {},
) {
  if (typeof binaryKey !== "string" || !Object.hasOwn(BINARIES, binaryKey)) {
    throw new Error(
      "usage: node scripts/run-ackplane.mjs <bridge|server|register-me|supervisor> [args...]",
    );
  }
  if (
    !Array.isArray(args) ||
    ![...args].every(
      (argument) => typeof argument === "string" && !argument.includes("\0"),
    )
  ) {
    throw new Error("arguments must be an array of strings without NUL bytes");
  }
  const binaryName = BINARIES[binaryKey];
  const workspace = resolve(cwd);
  const path = binaryPath(binaryName, platform, workspace);
  if (!exists(path)) {
    const packageName =
      binaryKey === "register-me" ? "ackplane-server" : binaryName;
    throw new Error(
      `${path} does not exist -- build it first: cargo build --release -p ${packageName} --bin ${binaryName}`,
    );
  }
  const childEnv = resolveEnv(resolve(workspace, ".env"), env);
  try {
    const result = spawn(path, args, {
      cwd: workspace,
      env: childEnv,
      shell: false,
      stdio: "inherit",
    });
    if (result.error) throw result.error;
    return result.signal ? 1 : (result.status ?? 1);
  } catch {
    throw new Error(`could not start ${binaryName}`);
  }
}

function main(argv) {
  const [command, ...args] = argv;
  process.exitCode = run(command, args);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(`run-ackplane: ${error.message}`);
    process.exitCode = 1;
  }
}
