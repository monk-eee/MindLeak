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
  assert.equal(pruneSupersededInstalls(path.dirname(destination)), 1);
  assert.equal(fs.existsSync(`${destination}.1234.old`), false);
  assert.equal(
    fs.existsSync(destination),
    true,
    "pruning must not remove the live binary",
  );

  fs.rmSync(root, { recursive: true, force: true });
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
