// Tests for the server installer. Run with: make script-test
//
// The path logic is what `.vscode/mcp.json` depends on: if the install location
// and the `${userHome}/.mindleak/bin` the config spells ever disagree, every
// window loses both MCP planes at once. So the decisions are pure and covered
// here rather than discovered on a broken fleet.
import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import {
  SERVERS,
  SUPERSEDED_SUFFIXES,
  executableName,
  installDirectory,
  installOne,
  pickBuild,
  pruneSupersededInstalls,
} from "./install-servers.mjs";

test("both servers are installed, because a window needs each plane", () => {
  assert.deepEqual(SERVERS, ["mindleak-mcp", "lodestar-mcp"]);
});

test("windows gets the extension it needs to spawn the binary", () => {
  assert.equal(executableName("mindleak-mcp", "win32"), "mindleak-mcp.exe");
  assert.equal(executableName("mindleak-mcp", "linux"), "mindleak-mcp");
  assert.equal(executableName("mindleak-mcp", "darwin"), "mindleak-mcp");
});

test("the install directory is the path mcp.json spells as ${userHome}/.mindleak/bin", () => {
  // Committed config cannot name a machine, so it uses ${userHome}. This is the
  // other half of that contract; if they drift, no server starts.
  assert.equal(
    installDirectory("/home/agent"),
    path.join("/home/agent", ".mindleak", "bin"),
  );
});

test("release wins over debug, and a missing build is reported rather than guessed", () => {
  const present = new Set([
    path.join("/repo", "target", "release", "mindleak-mcp"),
    path.join("/repo", "target", "debug", "mindleak-mcp"),
  ]);
  const exists = (p) => present.has(p);

  assert.equal(
    pickBuild("/repo", "mindleak-mcp", exists, "linux"),
    path.join("/repo", "target", "release", "mindleak-mcp"),
  );

  present.delete(path.join("/repo", "target", "release", "mindleak-mcp"));
  assert.equal(
    pickBuild("/repo", "mindleak-mcp", exists, "linux"),
    path.join("/repo", "target", "debug", "mindleak-mcp"),
  );

  // Nothing built: null, so the caller can say which build to run instead of
  // installing a file that is not there.
  assert.equal(pickBuild("/repo", "lodestar-mcp", exists, "linux"), null);
});

test("an existing server is moved aside rather than overwritten, and its bytes are replaced", () => {
  // Windows refuses to overwrite a running executable but allows renaming it.
  // Deleting instead would break the server that is live at that moment.
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "install-servers-"));
  const source = path.join(root, "new-server");
  const destination = path.join(root, "bin", "server");
  fs.writeFileSync(source, "new build");
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  fs.writeFileSync(destination, "old build");

  installOne(source, destination, 1234);

  assert.equal(fs.readFileSync(destination, "utf8"), "new build");
  assert.equal(fs.readFileSync(`${destination}.1234.old`, "utf8"), "old build");

  // A second install prunes what the first left behind.
  assert.deepEqual(pruneSupersededInstalls(path.dirname(destination)), {
    pruned: 1,
    held: 0,
  });
  assert.equal(fs.existsSync(`${destination}.1234.old`), false);
  assert.equal(
    fs.existsSync(destination),
    true,
    "pruning must not remove the live binary",
  );

  fs.rmSync(root, { recursive: true, force: true });
});

test("a binary renamed aside by a hand deploy is collected too, not just the installer's own", () => {
  // The collector matched only `.old`, the name `installOne` writes. A deploy
  // that copies a fresh build in by hand renames the live file to
  // `.superseded` for the same lock reason, so those were never collected:
  // measured 2026-08-14 at 8 orphans and 68.2 MiB in ~/.mindleak/bin.
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "install-servers-"));
  const bin = path.join(root, "bin");
  fs.mkdirSync(bin, { recursive: true });
  fs.writeFileSync(path.join(bin, "lodestar-mcp.exe"), "live");
  fs.writeFileSync(path.join(bin, "lodestar-mcp.exe.superseded"), "aside");
  fs.writeFileSync(
    path.join(bin, "mindleak-mcp.exe.1785576682021.old"),
    "aside",
  );
  fs.writeFileSync(path.join(bin, "lodestar-mcp-1551270.exe"), "pinned");

  assert.deepEqual(pruneSupersededInstalls(bin), { pruned: 2, held: 0 });

  assert.deepEqual(fs.readdirSync(bin).sort(), [
    "lodestar-mcp-1551270.exe",
    "lodestar-mcp.exe",
  ]);

  fs.rmSync(root, { recursive: true, force: true });
});

test("a set-aside binary that cannot be removed is reported as still held, not silently dropped", () => {
  // A running server keeps its old binary open and Windows then refuses the
  // delete. That refusal is the only portable evidence that the replaced code
  // is still being served, and it used to be swallowed by a bare `catch`, so a
  // fixed binary on disk read as a completed install while every live process
  // carried on running the defect. Simulated with an entry `rmSync` refuses on
  // every platform, because a genuinely locked file is not reproducible on all
  // of them.
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "install-servers-"));
  const bin = path.join(root, "bin");
  fs.mkdirSync(bin, { recursive: true });
  fs.writeFileSync(path.join(bin, "lodestar-mcp.exe"), "live");
  fs.writeFileSync(path.join(bin, "mindleak-mcp.exe.1.old"), "collectable");
  fs.mkdirSync(path.join(bin, "lodestar-mcp.exe.2.old"));
  fs.writeFileSync(
    path.join(bin, "lodestar-mcp.exe.2.old", "holds-it-open"),
    "x",
  );

  assert.deepEqual(pruneSupersededInstalls(bin), { pruned: 1, held: 1 });

  // The collectable one is gone, the held one is left for a later run, and the
  // live binary is untouched either way.
  assert.deepEqual(fs.readdirSync(bin).sort(), [
    "lodestar-mcp.exe",
    "lodestar-mcp.exe.2.old",
  ]);

  fs.rmSync(root, { recursive: true, force: true });
});

test("every suffix the collector takes is one something in this repository writes", () => {
  // Guards the guard: widening this list is how the collector would start
  // deleting a live binary rather than a set-aside copy.
  assert.deepEqual(SUPERSEDED_SUFFIXES, [".old", ".superseded"]);
});

test("installing into an absent directory creates it", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "install-servers-"));
  const source = path.join(root, "server");
  fs.writeFileSync(source, "build");
  const destination = path.join(root, "never", "made", "server");

  installOne(source, destination);

  assert.equal(fs.readFileSync(destination, "utf8"), "build");
  fs.rmSync(root, { recursive: true, force: true });
});
