import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";

import { packageIndustrialBundle } from "./industrial-bundle.mjs";
import {
  INDUSTRIAL_BINARIES,
  executableName,
  readIndustrialBundle,
} from "./install-servers.mjs";

function fixture(context, target = "x86_64-unknown-linux-gnu") {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "industrial-bundle-"));
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const platform = target.includes("windows")
    ? "win32"
    : target.includes("apple")
      ? "darwin"
      : "linux";
  const arch = target.startsWith("aarch64") ? "arm64" : "x64";
  const directory = path.join(root, "custom-target", target, "release");
  fs.mkdirSync(directory, { recursive: true });
  fs.mkdirSync(path.join(root, "scripts"));
  fs.copyFileSync(
    new URL("./install-servers.mjs", import.meta.url),
    path.join(root, "scripts", "install-servers.mjs"),
  );
  fs.writeFileSync(path.join(root, "LICENSE"), "test fixture license");
  for (const binary of INDUSTRIAL_BINARIES) {
    fs.writeFileSync(
      path.join(directory, executableName(binary, platform)),
      `binary:${binary}`,
    );
  }
  const calls = [];
  let archiveContents;
  const execute = (command, args) => {
    calls.push({ command, args });
    if (command === "git") return args[0] === "status" ? "" : "a".repeat(40);
    if (command === "cargo" && args[0] === "metadata")
      return JSON.stringify({
        target_directory: path.join(root, "custom-target"),
        packages: INDUSTRIAL_BINARIES.map((name) => ({
          name: name === "register-me" ? "ackplane-server" : name,
          version: "0.1.7-alpha",
        })),
      });
    if (command === "cargo" && args[0] === "build") return "";
    const binary = INDUSTRIAL_BINARIES.find(
      (name) => path.basename(command) === executableName(name, platform),
    );
    if (binary) {
      return binary.endsWith("-mcp")
        ? JSON.stringify({
            id: 1,
            result: { serverInfo: { name: binary, version: "0.1.7-alpha" } },
          })
        : "usage: fixture";
    }
    assert.equal(command, process.execPath);
    const source = args[args.indexOf("--source") + 1];
    archiveContents = readIndustrialBundle(source, platform, arch);
    assert.deepEqual(
      fs.readdirSync(source).sort(),
      [
        ...INDUSTRIAL_BINARIES.map((name) => executableName(name, platform)),
        "install.mjs",
        "industrial-manifest.json",
        "LICENSE",
        "README.txt",
      ].sort(),
    );
    assert.match(
      fs.readFileSync(path.join(source, "README.txt"), "utf8"),
      /not publisher authenticity/,
    );
    fs.writeFileSync(args[args.indexOf("--out") + 1], "archive fixture");
    return "";
  };
  return {
    root,
    calls,
    directory,
    execute,
    options: { workspace: root, target, out: "dist/industrial.zip" },
    archived: () => archiveContents,
  };
}

test("one clean build packages exactly six binaries with revision, version and hashes", (context) => {
  const setup = fixture(context);
  const result = packageIndustrialBundle(setup.options, setup.execute);
  assert.equal(result.manifest.revision, "a".repeat(40));
  assert.equal(result.manifest.version, "0.1.7-alpha");
  assert.deepEqual(result.manifest, setup.archived().manifest);
  assert.equal(fs.readFileSync(result.destination, "utf8"), "archive fixture");
  assert.deepEqual(fs.readdirSync(path.dirname(result.destination)), [
    "industrial.zip",
  ]);
  const build = setup.calls.find(
    (call) => call.command === "cargo" && call.args[0] === "build",
  );
  for (const value of [
    "--locked",
    "--release",
    "--target",
    "ackplane-server",
    "ackplane-workctl",
    "mindleak-mcp/federation-client,lodestar-mcp/federation-client",
  ]) {
    assert.ok(build.args.includes(value));
  }
  assert.ok(!build.args.includes("register-me"));
});

test("target platforms select native filenames and architecture without host guessing", (context) => {
  for (const target of [
    "x86_64-pc-windows-msvc",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
  ]) {
    const setup = fixture(context, target);
    const result = packageIndustrialBundle(setup.options, setup.execute);
    assert.equal(
      result.manifest.arch,
      target.startsWith("aarch64") ? "arm64" : "x64",
    );
    assert.equal(
      result.manifest.files[0].name.endsWith(".exe"),
      target.includes("windows"),
    );
  }
});

test("dirty source refuses before building or creating an archive", (context) => {
  const setup = fixture(context);
  assert.throws(
    () =>
      packageIndustrialBundle(setup.options, (command, args) => {
        if (command === "git" && args[0] === "status") return " M source.rs";
        return setup.execute(command, args);
      }),
    /clean committed checkout/,
  );
  assert.equal(setup.calls.length, 0);
  assert.ok(!fs.existsSync(path.join(setup.root, "dist")));
});

test("source changes during a build cannot be labeled with the original revision", (context) => {
  const setup = fixture(context);
  let built = false;
  assert.throws(
    () =>
      packageIndustrialBundle(setup.options, (command, args) => {
        if (command === "cargo" && args[0] === "build") built = true;
        if (command === "git" && args[0] === "rev-parse" && built)
          return "b".repeat(40);
        return setup.execute(command, args);
      }),
    /revision changed/,
  );
  assert.ok(!fs.existsSync(path.join(setup.root, "dist")));
});

test("missing build output and packaging failure leave no published archive", (context) => {
  for (const failure of ["missing", "packager"]) {
    const setup = fixture(context);
    if (failure === "missing")
      fs.unlinkSync(path.join(setup.directory, "ackplane-workctl"));
    assert.throws(() =>
      packageIndustrialBundle(setup.options, (command, args) => {
        if (failure === "packager" && command === process.execPath)
          throw new Error("packaging failed");
        return setup.execute(command, args);
      }),
    );
    assert.deepEqual(fs.readdirSync(path.join(setup.root, "dist")), []);
  }
});

test("existing bundles are never overwritten", (context) => {
  const setup = fixture(context);
  const result = packageIndustrialBundle(setup.options, setup.execute);
  assert.throws(
    () => packageIndustrialBundle(setup.options, setup.execute),
    /already exists/,
  );
  assert.equal(fs.readFileSync(result.destination, "utf8"), "archive fixture");
});

test("dirty source discovered after packaging removes staging without publishing", (context) => {
  const setup = fixture(context);
  let packaged = false;
  assert.throws(
    () =>
      packageIndustrialBundle(setup.options, (command, args) => {
        if (command === "git" && args[0] === "status" && packaged)
          return " M changed.rs";
        const result = setup.execute(command, args);
        if (command === process.execPath) packaged = true;
        return result;
      }),
    /clean committed checkout/,
  );
  assert.deepEqual(fs.readdirSync(path.join(setup.root, "dist")), []);
});

test("an output published by another process is preserved at final publication", (context) => {
  const setup = fixture(context);
  const destination = path.join(setup.root, setup.options.out);
  assert.throws(
    () =>
      packageIndustrialBundle(setup.options, (command, args) => {
        const result = setup.execute(command, args);
        if (command === process.execPath)
          fs.writeFileSync(destination, "another publisher");
        return result;
      }),
    /EEXIST/,
  );
  assert.equal(fs.readFileSync(destination, "utf8"), "another publisher");
  assert.deepEqual(fs.readdirSync(path.dirname(destination)), [
    "industrial.zip",
  ]);
});

test("mismatched package versions refuse before building a mixed bundle", (context) => {
  const setup = fixture(context);
  assert.throws(
    () =>
      packageIndustrialBundle(setup.options, (command, args) => {
        const result = setup.execute(command, args);
        if (command !== "cargo" || args[0] !== "metadata") return result;
        const metadata = JSON.parse(result);
        metadata.packages[5].version = "99.0.0";
        return JSON.stringify(metadata);
      }),
    /one shared version/,
  );
  assert.ok(
    !setup.calls.some(
      (call) => call.command === "cargo" && call.args[0] === "build",
    ),
  );
  assert.ok(!fs.existsSync(path.join(setup.root, "dist")));
});

test("an unlaunchable host command prevents archive publication", (context) => {
  const setup = fixture(context);
  assert.throws(
    () =>
      packageIndustrialBundle(setup.options, (command, args) => {
        if (path.basename(command) === "ackplane-supervisor")
          throw new Error("cannot execute host binary");
        return setup.execute(command, args);
      }),
    /cannot execute/,
  );
  assert.deepEqual(fs.readdirSync(path.join(setup.root, "dist")), []);
});

test("an MCP binary identifying another version cannot enter the bundle", (context) => {
  const setup = fixture(context);
  assert.throws(
    () =>
      packageIndustrialBundle(setup.options, (command, args) => {
        if (path.basename(command) === "ackplane-mcp") {
          return JSON.stringify({
            id: 1,
            result: { serverInfo: { name: "ackplane-mcp", version: "0.0.1" } },
          });
        }
        return setup.execute(command, args);
      }),
    /unexpected installed MCP identity/,
  );
  assert.deepEqual(fs.readdirSync(path.join(setup.root, "dist")), []);
});
