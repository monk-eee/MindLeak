import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { isolatedGit } from "./adr-files.mjs";
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
            result: {
              serverInfo: {
                name: binary,
                version:
                  binary === "ackplane-mcp"
                    ? "0.1.7-alpha"
                    : "0.1.7-alpha+aaaaaaaaaaaa",
              },
            },
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

test("the bundle CLI reports help when invoked through a linked directory", (context) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "bundle-cli-link-"));
  context.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const alias = path.join(root, "scripts");
  fs.symlinkSync(
    path.dirname(fileURLToPath(import.meta.url)),
    alias,
    process.platform === "win32" ? "junction" : "dir",
  );
  const result = spawnSync(
    process.execPath,
    [path.join(alias, "industrial-bundle.mjs"), "--help"],
    {
      encoding: "utf8",
      timeout: 10_000,
    },
  );
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Usage: node scripts\/industrial-bundle.mjs/);
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

test("archive source checks ignore inherited Git pointers and refuse dirty or unreadable input", (context) => {
  const candidate = fixture(context).root;
  const foreign = fixture(context).root;
  for (const directory of [candidate, foreign]) {
    assert.notEqual(isolatedGit(["init", "--quiet"], directory), null);
    fs.writeFileSync(path.join(directory, "source.txt"), `${directory}\n`);
    assert.notEqual(isolatedGit(["add", "."], directory), null);
    assert.notEqual(
      isolatedGit(
        [
          "-c",
          "user.name=Bundle Test",
          "-c",
          "user.email=bundle@example.invalid",
          "-c",
          "commit.gpgsign=false",
          "commit",
          "--quiet",
          "-m",
          "fixture",
        ],
        directory,
      ),
      null,
    );
  }
  const revision = isolatedGit(["rev-parse", "HEAD"], candidate);
  const metadata = isolatedGit(["rev-parse", "--absolute-git-dir"], foreign);
  const program = `
    import { execFileSync } from "node:child_process";
    import { packageIndustrialBundle } from ${JSON.stringify(new URL("./industrial-bundle.mjs", import.meta.url).href)};
    let revision;
    try {
      packageIndustrialBundle({ workspace: process.argv[1] }, (command, args, options) => {
        if (command === "git") {
          const output = execFileSync(command, args, options);
          if (args[0] === "rev-parse") revision = output.trim();
          return output;
        }
        const childRevision = execFileSync("git", ["rev-parse", "HEAD"], options).trim();
        console.log(JSON.stringify({ revision, childRevision }));
        throw new Error("probe stopped before Cargo");
      });
    } catch (error) {
      if (error.message !== "probe stopped before Cargo") {
        console.error(error.message);
        process.exitCode = 1;
      }
    }
  `;
  const inspect = () =>
    spawnSync(
      process.execPath,
      ["--input-type=module", "-e", program, candidate],
      {
        encoding: "utf8",
        timeout: 10_000,
        env: {
          ...process.env,
          GIT_DIR: metadata,
          GIT_COMMON_DIR: metadata,
          GIT_WORK_TREE: foreign,
          GIT_INDEX_FILE: path.join(metadata, "index"),
          GIT_OBJECT_DIRECTORY: path.join(metadata, "objects"),
          GIT_ALTERNATE_OBJECT_DIRECTORIES: path.join(metadata, "objects"),
        },
      },
    );

  // Foreign Git pointers mislabeled archives and hid dirty source from the build guard.
  const clean = inspect();
  assert.equal(clean.status, 0, clean.stderr);
  assert.deepEqual(JSON.parse(clean.stdout), {
    revision,
    childRevision: revision,
  });
  for (const file of ["source.txt", "untracked.txt"]) {
    fs.writeFileSync(path.join(candidate, file), "changed\n");
    const dirty = inspect();
    assert.equal(dirty.status, 1, dirty.stderr);
    assert.equal(dirty.stdout, "");
    assert.match(dirty.stderr, /clean committed checkout/);
    if (file === "source.txt") {
      fs.writeFileSync(path.join(candidate, file), `${candidate}\n`);
    } else {
      fs.rmSync(path.join(candidate, file));
    }
  }
  fs.rmSync(path.join(candidate, ".git"), { recursive: true, force: true });
  const unreadable = inspect();
  assert.equal(unreadable.status, 1, unreadable.stderr);
  assert.equal(unreadable.stdout, "");
  assert.match(unreadable.stderr, /cannot read Industrial source checkout/);
  assert.ok(!fs.existsSync(path.join(candidate, "dist")));
  assert.equal(isolatedGit(["status", "--porcelain"], foreign), "");
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

test("a custom archive path does not make the builder's own staging look like changed source", (context) => {
  const setup = fixture(context);
  execFileSync("git", ["init", "--quiet", setup.root]);
  fs.writeFileSync(
    path.join(setup.root, ".git", "info", "exclude"),
    "/custom-target/\n/scripts/\n/LICENSE\n",
  );
  const result = packageIndustrialBundle(
    { ...setup.options, out: "host release.zip" },
    (command, args) => {
      if (command === "git" && args[0] === "status") {
        return execFileSync(command, args, {
          cwd: setup.root,
          encoding: "utf8",
        });
      }
      return setup.execute(command, args);
    },
  );
  assert.equal(fs.readFileSync(result.destination, "utf8"), "archive fixture");
  assert.ok(
    !fs
      .readdirSync(setup.root)
      .some((name) => name.startsWith(".industrial-bundle-")),
  );
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

test("an MCP binary with a missing or foreign build revision cannot enter the bundle", (context) => {
  for (const binary of ["mindleak-mcp", "lodestar-mcp"]) {
    for (const version of ["0.1.7-alpha", "0.1.7-alpha+bbbbbbbbbbbb"]) {
      const setup = fixture(context);
      // Version-only smoke checks accepted executables built with a foreign source identity.
      assert.throws(
        () =>
          packageIndustrialBundle(setup.options, (command, args, options) => {
            if (path.basename(command) === binary) {
              return JSON.stringify({
                id: 1,
                result: { serverInfo: { name: binary, version } },
              });
            }
            return setup.execute(command, args, options);
          }),
        /unexpected installed MCP source revision/,
      );
      assert.deepEqual(fs.readdirSync(path.join(setup.root, "dist")), []);
    }
  }
});
