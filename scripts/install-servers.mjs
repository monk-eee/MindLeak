// Install the built MCP servers where every window can reach them.
//
// A window must be rooted at the worktree it edits, or the path a save reports
// cannot be made repository-relative and the file never reaches the graph
// (ADR-0073). Binding the servers to `${workspaceFolder}/target/release` would
// demand a release build in every worktree — measured at 56 worktrees and 184 GB
// of build output already, with only 15 holding a server binary.
//
// So the servers are installed once per machine, outside every worktree, at a
// stable version-independent path. Copying them into each worktree's own
// `target/` is deliberately NOT done: cargo's fingerprints would still read
// fresh, so that worktree would never rebuild and would silently serve a binary
// that does not match its source.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

export const SERVERS = ["mindleak-mcp", "lodestar-mcp"];
const INDUSTRIAL_BINARIES = [
  ...SERVERS,
  "ackplane-mcp",
  "ackplane-supervisor",
  "register-me",
  "ackplane-workctl",
];

/** Executable name for a platform. Windows needs the extension to spawn. */
export function executableName(name, platform = process.platform) {
  return platform === "win32" ? `${name}.exe` : name;
}

/**
 * Where the servers are installed. Must match the shared-install location that
 * `resolveBinaryPath` in editors/vscode/src/util.ts prefers over a worktree
 * build, so the extension and this installer agree on one path.
 */
export function installDirectory(home = os.homedir()) {
  return path.join(home, ".mindleak", "bin");
}

/**
 * The build to install from: release first, then debug.
 *
 * Mirrors the precedence the extension already uses to find a server
 * (`resolveBinaryPath` in editors/vscode/src/util.ts) so a developer does not
 * have to hold two different rules about which build wins.
 */
export function pickBuild(
  workspace,
  name,
  exists = fs.existsSync,
  platform = process.platform,
  {
    profiles = ["release", "debug"],
    targetDirectory = path.join(workspace, "target"),
  } = {},
) {
  const exe = executableName(name, platform);
  for (const profile of profiles) {
    const candidate = path.join(targetDirectory, profile, exe);
    if (exists(candidate)) {
      return candidate;
    }
  }
  return null;
}

/**
 * Replace a destination that may be running.
 *
 * Windows refuses to overwrite a live executable but does allow renaming one,
 * and VS Code respawns a killed server within a second and re-locks it. So the
 * old file is moved aside rather than deleted: the running process keeps its
 * handle, and the next spawn picks up the new binary.
 */
export function installOne(source, destination, now = Date.now()) {
  const directory = path.dirname(destination);
  fs.mkdirSync(directory, { recursive: true });
  const staging = fs.mkdtempSync(path.join(directory, ".install-"));
  const candidate = path.join(staging, path.basename(destination));
  let previous = null;
  try {
    fs.copyFileSync(source, candidate);
    const stamped = new Date();
    fs.utimesSync(candidate, stamped, stamped);
    if (process.platform !== "win32") {
      fs.chmodSync(candidate, 0o755);
    }
    if (fs.existsSync(destination)) {
      if (!fs.lstatSync(destination).isFile()) {
        throw new Error(`installed path is not a regular file: ${destination}`);
      }
      const backup = `${destination}.${now}.old`;
      if (fs.existsSync(backup)) {
        throw new Error(`a previous install already uses ${backup}`);
      }
      fs.renameSync(destination, backup);
      previous = backup;
    }
    try {
      fs.renameSync(candidate, destination);
    } catch (error) {
      if (previous) fs.renameSync(previous, destination);
      throw error;
    }
  } finally {
    fs.rmSync(staging, { recursive: true, force: true });
  }
}

/**
 * Suffixes a set-aside binary can carry.
 *
 * `installOne` writes `.old`; a deploy that copies a fresh build in by hand
 * renames the live file to `.superseded` for the same reason, so both land in
 * this directory and both are this collector's to take.
 */
export const SUPERSEDED_SUFFIXES = [".old", ".superseded"];

/**
 * Delete the binaries earlier installs and deploys set aside, once unlocked.
 *
 * A set-aside binary that refuses to delete is not just "try again later": a
 * process still holds it, which is the only portable evidence available here
 * that a server is still running the code this install replaced. Report it.
 */
export function pruneSupersededInstalls(directory) {
  let pruned = 0;
  let held = 0;
  for (const entry of fs.readdirSync(directory)) {
    if (!SUPERSEDED_SUFFIXES.some((suffix) => entry.endsWith(suffix))) continue;
    try {
      fs.rmSync(path.join(directory, entry));
      pruned += 1;
    } catch {
      held += 1;
    }
  }
  return { pruned, held };
}

function main() {
  const { values } = parseArgs({
    options: {
      profile: { type: "string", default: "local" },
      prune: { type: "boolean", default: false },
      help: { type: "boolean", short: "h" },
    },
  });
  if (values.help) {
    console.log(
      "Usage: node scripts/install-servers.mjs [--profile local|industrial] [--prune]\n" +
        "Local (default): install mindleak-mcp and lodestar-mcp, preferring release over debug.\n" +
        "Industrial: install all six host binaries from target/release; no debug fallback.\n" +
        "CARGO_TARGET_DIR selects a different build directory for either profile.\n" +
        "This does not install or start the shared Ackplane/Bridge deployment.",
    );
    return;
  }
  if (!["local", "industrial"].includes(values.profile)) {
    throw new Error("--profile must be local or industrial");
  }
  if (values.prune && values.profile !== "local") {
    throw new Error(
      "--prune collects shared installs and cannot select an Industrial profile",
    );
  }
  const directory = installDirectory();

  // Reachable on its own because the collector used to run only after a full
  // install, and a deploy that copies a fresh build in by hand never performs
  // one — which is how 68 MiB of set-aside binaries accumulated unnoticed.
  if (values.prune) {
    const collected = pruneSupersededInstalls(directory);
    reportPruned(collected, directory);
    reportHeld(collected.held);
    return;
  }

  const workspace = execFileSync("git", ["rev-parse", "--show-toplevel"], {
    encoding: "utf8",
  }).trim();

  const industrial = values.profile === "industrial";
  const targetDirectory = path.resolve(
    workspace,
    process.env.CARGO_TARGET_DIR || "target",
  );
  const binaries = industrial ? INDUSTRIAL_BINARIES : SERVERS;
  const builds = binaries.map((name) => ({
    name,
    source: pickBuild(workspace, name, fs.existsSync, process.platform, {
      profiles: industrial ? ["release"] : ["release", "debug"],
      targetDirectory,
    }),
  }));
  const missing = builds
    .filter(({ source }) => !source || !fs.statSync(source).isFile())
    .map(({ name }) => name);
  if (missing.length > 0) {
    const packages = industrial
      ? binaries.map((name) =>
          name === "register-me" ? "ackplane-server" : name,
        )
      : missing;
    const features = industrial
      ? " --features mindleak-mcp/federation-client,lodestar-mcp/federation-client"
      : "";
    console.error(
      `install-servers: no ${industrial ? "release " : ""}build found for ${missing.join(", ")}.\n` +
        `  Build them first: cargo build --locked --release -p ${packages.join(" -p ")}${features}`,
    );
    process.exitCode = 1;
    return;
  }

  for (const { source } of builds) {
    fs.accessSync(source, fs.constants.R_OK);
  }
  for (const { name, source } of builds) {
    const destination = path.join(directory, executableName(name));
    installOne(source, destination);
    console.log(
      `install-servers: ${path.relative(workspace, source)} -> ${destination}`,
    );
  }
  const { pruned, held } = pruneSupersededInstalls(directory);
  if (pruned > 0) {
    console.log(`install-servers: removed ${supersededCount(pruned)}`);
  }
  console.log(
    industrial
      ? "install-servers: Industrial host binaries installed; stop and restart the companion and consumers to use this build. No services were started or credentials changed."
      : "install-servers: restart the MCP servers (or reload the window) so clients pick these up",
  );
  reportHeld(held);
}

const supersededCount = (n) => `${n} superseded binar${n === 1 ? "y" : "ies"}`;

/**
 * Name the servers that are demonstrably still serving the replaced code.
 *
 * Windows refuses to delete a binary a live process holds, so a held count is a
 * direct measurement, not a guess — and the difference matters, because a fixed
 * binary on disk changes nothing until every process running the old one stops.
 * Unix unlinks a running binary happily, so a quiet result there means "no
 * evidence", never "nothing is running".
 */
function reportHeld(held) {
  if (held < 1) return;
  console.log(
    `install-servers: ${supersededCount(held)} could not be removed because a process still holds ${
      held === 1 ? "it" : "them"
    }.\n` +
      `  Those servers are still running the code this install replaced, so the change is not live until they restart.`,
  );
}

function reportPruned({ pruned }, directory) {
  console.log(
    pruned > 0
      ? `install-servers: removed ${supersededCount(pruned)} from ${directory}`
      : `install-servers: nothing to collect in ${directory}; anything still held by a running server is taken on a later run`,
  );
}

// Run only when invoked directly, never when imported by the tests. Compared
// through `fileURLToPath` because a naive `file://${argv[1]}` comparison never
// matches on Windows, where the path carries a drive letter and backslashes —
// which would leave the CLI silently doing nothing.
if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    main();
  } catch (error) {
    console.error(`install-servers: ${error.message}`);
    process.exitCode = 1;
  }
}
