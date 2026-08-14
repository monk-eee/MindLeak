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

export const SERVERS = ["mindleak-mcp", "lodestar-mcp"];

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
) {
  const exe = executableName(name, platform);
  for (const profile of ["release", "debug"]) {
    const candidate = path.join(workspace, "target", profile, exe);
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
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  if (fs.existsSync(destination)) {
    fs.renameSync(destination, `${destination}.${now}.old`);
  }
  fs.copyFileSync(source, destination);
  // Copy preserves the source mtime on some platforms, which makes a fresh
  // install look older than what it replaced. Stamp it so "which is newer" stays
  // answerable.
  const stamped = new Date();
  fs.utimesSync(destination, stamped, stamped);
  if (process.platform !== "win32") {
    fs.chmodSync(destination, 0o755);
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

/** Delete the binaries earlier installs and deploys set aside, once unlocked. */
export function pruneSupersededInstalls(directory) {
  let pruned = 0;
  for (const entry of fs.readdirSync(directory)) {
    if (!SUPERSEDED_SUFFIXES.some((suffix) => entry.endsWith(suffix))) continue;
    try {
      fs.rmSync(path.join(directory, entry));
      pruned += 1;
    } catch {
      // Still held by a running server; the next prune will get it.
    }
  }
  return pruned;
}

function main() {
  const directory = installDirectory();

  // Reachable on its own because the collector used to run only after a full
  // install, and a deploy that copies a fresh build in by hand never performs
  // one — which is how 68 MiB of set-aside binaries accumulated unnoticed.
  if (process.argv.slice(2).includes("--prune")) {
    reportPruned(pruneSupersededInstalls(directory), directory);
    return;
  }

  const workspace = execFileSync("git", ["rev-parse", "--show-toplevel"], {
    encoding: "utf8",
  }).trim();

  const missing = SERVERS.filter((name) => !pickBuild(workspace, name));
  if (missing.length > 0) {
    console.error(
      `install-servers: no build found for ${missing.join(", ")}.\n` +
        `  Build them first:  cargo build --release -p ${missing.join(" -p ")}`,
    );
    process.exitCode = 1;
    return;
  }

  for (const name of SERVERS) {
    const source = pickBuild(workspace, name);
    const destination = path.join(directory, executableName(name));
    installOne(source, destination);
    console.log(
      `install-servers: ${path.relative(workspace, source)} -> ${destination}`,
    );
  }
  const pruned = pruneSupersededInstalls(directory);
  if (pruned > 0) {
    console.log(`install-servers: removed ${supersededCount(pruned)}`);
  }
  console.log(
    "install-servers: restart the MCP servers (or reload the window) so clients pick these up",
  );
}

const supersededCount = (n) => `${n} superseded binar${n === 1 ? "y" : "ies"}`;

function reportPruned(pruned, directory) {
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
  main();
}
