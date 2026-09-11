import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { X509Certificate } from "node:crypto";
import fs from "node:fs";
import { syncBuiltinESMExports } from "node:module";
import { tmpdir } from "node:os";
import { dirname, isAbsolute, join } from "node:path";
import test from "node:test";
import { rootCertificates } from "node:tls";
import { fileURLToPath } from "node:url";
import { inspect } from "node:util";

import { composeCommand } from "./ackplane-compose.mjs";
import * as lifecycle from "./ackplane-compose.mjs";

const CA = Buffer.from(rootCertificates[0]);
const SALT = Buffer.from("nonsecret-legacy-bridge-salt");
const CA_SOURCE = "ackplane:/tls/ca.crt";
const SALT_SOURCE = "bridge:/var/lib/ackplane-bridge/salt";

function fixture(context) {
  const root = fs.mkdtempSync(join(tmpdir(), "ackplane-compose-test-"));
  const directory = join(root, "config with spaces ; $literal");
  fs.mkdirSync(directory);
  const scenario = {
    root,
    directory,
    caPath: join(directory, "ackplane-dev-ca.pem"),
    saltPath: join(directory, "bridge.salt"),
    env: { MINDLEAK_COMPOSE_BIN: "podman" },
    calls: [],
    sources: new Map([
      [CA_SOURCE, CA],
      [SALT_SOURCE, SALT],
    ]),
  };
  const stagingDirectories = new Set();
  context.after(() => {
    try {
      for (const staging of stagingDirectories) {
        assert.equal(fs.existsSync(staging), false, "owned staging is removed");
      }
    } finally {
      for (const staging of stagingDirectories) {
        fs.rmSync(staging, { recursive: true, force: true });
      }
      fs.rmSync(root, { recursive: true, force: true });
    }
  });
  scenario.copy = (source, destination) =>
    fs.writeFileSync(destination, scenario.sources.get(source));
  scenario.run = (command, args, options) => {
    assert.equal(command, scenario.env.MINDLEAK_COMPOSE_BIN);
    assert.equal(args.length, 4);
    assert.deepEqual(args.slice(0, 2), ["compose", "cp"]);
    assert.ok(scenario.sources.has(args[2]), "only public CA and bridge salt");
    assert.ok(isAbsolute(args[3]), "staging uses an absolute native path");
    assert.equal(options.env, scenario.env);
    assert.notEqual(options.shell, true);
    assert.deepEqual(options.stdio, ["ignore", "pipe", "pipe"]);
    const staging = dirname(args[3]);
    assert.notEqual(staging, directory);
    stagingDirectories.add(staging);
    scenario.calls.push(args);
    scenario.copy(args[2], args[3]);
  };
  scenario.prepare = () =>
    lifecycle.prepare(directory, { env: scenario.env, run: scenario.run });
  return scenario;
}

test("composeCommand honors an explicit binary", () => {
  assert.equal(
    composeCommand({ MINDLEAK_COMPOSE_BIN: "podman" }, () =>
      assert.fail("an explicit binary should not be probed"),
    ),
    "podman",
  );
});

test("composeCommand selects the first available implementation", () => {
  const probes = [];
  const command = composeCommand({}, (candidate) => {
    probes.push(candidate);
    if (candidate === "docker") {
      throw new Error("docker unavailable");
    }
  });

  assert.equal(command, "podman");
  assert.deepEqual(probes, ["docker", "podman"]);
});

test("composeCommand explains when neither implementation is available", () => {
  assert.throws(
    () =>
      composeCommand({}, () => {
        throw new Error("unavailable");
      }),
    /neither docker nor podman compose is available/,
  );
});

test("prepare stages both exact public sources before publishing validated trust", (context) => {
  const scenario = fixture(context);
  const certificate = new X509Certificate(CA);
  assert.equal(certificate.ca, true);
  assert.equal(certificate.verify(certificate.publicKey), true);
  fs.writeFileSync(join(scenario.directory, "untouched"), "keep");
  scenario.copy = (source, destination) => {
    assert.deepEqual(fs.readdirSync(scenario.directory), ["untouched"]);
    if (process.platform !== "win32") {
      assert.equal(fs.statSync(dirname(destination)).mode & 0o777, 0o700);
    }
    fs.writeFileSync(destination, scenario.sources.get(source));
  };

  assert.deepEqual(scenario.prepare(), {
    caPath: scenario.caPath,
    saltPath: scenario.saltPath,
  });
  assert.deepEqual(
    scenario.calls.map((args) => args[2]),
    [CA_SOURCE, SALT_SOURCE],
  );
  assert.equal(dirname(scenario.calls[0][3]), dirname(scenario.calls[1][3]));
  assert.deepEqual(fs.readFileSync(scenario.caPath), CA);
  assert.deepEqual(fs.readFileSync(scenario.saltPath), SALT);
  if (process.platform !== "win32") {
    for (const target of [scenario.caPath, scenario.saltPath]) {
      assert.equal(fs.statSync(target).mode & 0o777, 0o600);
    }
  }
  assert.doesNotMatch(
    JSON.stringify(scenario.calls),
    /private|\.key|seed|enroll|approve/,
  );
});

test("prepare creates a missing config directory only after staging both files", (context) => {
  const scenario = fixture(context);
  fs.rmdirSync(scenario.directory);
  scenario.copy = (source, destination) => {
    assert.equal(fs.existsSync(scenario.directory), false);
    fs.writeFileSync(destination, scenario.sources.get(source));
  };
  scenario.prepare();
  assert.deepEqual(fs.readFileSync(scenario.caPath), CA);
  assert.deepEqual(fs.readFileSync(scenario.saltPath), SALT);
});

for (const length of [1, 24, 64, 4096]) {
  test(`prepare accepts a nonempty ${length}-byte legacy salt`, (context) => {
    const scenario = fixture(context);
    const salt = Buffer.alloc(length, 0x61);
    scenario.sources.set(SALT_SOURCE, salt);
    scenario.prepare();
    assert.deepEqual(fs.readFileSync(scenario.saltPath), salt);
  });
}

test("prepare rejects relative, ambiguous, and unsupported destinations before compose", () => {
  const invalid = [
    undefined,
    null,
    42,
    "",
    ".",
    "../config",
    "config/path",
    "~/config",
    "C:config",
    "\\config",
    "file:///tmp/config",
    "https://example.invalid/config",
    `${tmpdir()}\0config`,
  ];
  if (process.platform === "win32") {
    invalid.push("/config", "\\\\server\\share\\config", "\\\\?\\C:\\config");
  } else {
    invalid.push(
      "C:\\config",
      "\\\\server\\share\\config",
      "//server/share/config",
    );
  }
  for (const directory of invalid) {
    assert.throws(
      () =>
        lifecycle.prepare(directory, { run: () => assert.fail("no compose") }),
      /absolute.*(?:local|config)|unsupported/i,
    );
  }
});

for (const [label, source, data, reason] of [
  [
    "invalid CA",
    CA_SOURCE,
    Buffer.from("invalid-public-certificate"),
    /CA certificate/i,
  ],
  ["empty CA", CA_SOURCE, Buffer.alloc(0), /nonempty|empty/i],
  [
    "oversized CA",
    CA_SOURCE,
    Buffer.alloc(65537),
    /65536|large|limit|bounded/i,
  ],
  ["empty salt", SALT_SOURCE, Buffer.alloc(0), /nonempty|empty/i],
  [
    "oversized salt",
    SALT_SOURCE,
    Buffer.alloc(4097),
    /4096|large|limit|bounded/i,
  ],
]) {
  test(`prepare refuses ${label} without creating a missing destination`, (context) => {
    const scenario = fixture(context);
    fs.rmdirSync(scenario.directory);
    scenario.sources.set(source, data);
    assert.throws(scenario.prepare, reason);
    assert.equal(fs.existsSync(scenario.directory), false);
    assert.equal(
      scenario.calls.length,
      2,
      "both copies precede validation/publication",
    );
  });
}

for (const failedSource of [CA_SOURCE, SALT_SOURCE]) {
  test(`prepare sanitizes a failed copy of ${failedSource} and preserves existing files`, (context) => {
    const scenario = fixture(context);
    fs.writeFileSync(scenario.caPath, CA);
    const before = fs.statSync(scenario.caPath);
    scenario.copy = (source, destination) => {
      fs.writeFileSync(destination, scenario.sources.get(source));
      if (source === failedSource) {
        const error = new Error(`DO-NOT-LEAK ${SALT} ${CA}`);
        error.stderr = Buffer.from("DO-NOT-LEAK child output");
        throw error;
      }
    };
    assert.throws(scenario.prepare, (error) => {
      assert.match(error.message, /compose cp failed/i);
      assert.ok(error.message.includes(failedSource));
      assert.doesNotMatch(
        inspect(error),
        /DO-NOT-LEAK|BEGIN CERTIFICATE|nonsecret-legacy/,
      );
      return true;
    });
    assert.deepEqual(fs.readdirSync(scenario.directory), [
      "ackplane-dev-ca.pem",
    ]);
    assert.deepEqual(fs.readFileSync(scenario.caPath), CA);
    assert.equal(fs.statSync(scenario.caPath).mtimeMs, before.mtimeMs);
  });
}

for (const source of [CA_SOURCE, SALT_SOURCE]) {
  test(`prepare refuses a nonregular staged source for ${source}`, (context) => {
    const scenario = fixture(context);
    scenario.copy = (current, destination) => {
      if (current === source) fs.mkdirSync(destination);
      else fs.writeFileSync(destination, scenario.sources.get(current));
    };
    assert.throws(scenario.prepare, /regular file/i);
    assert.deepEqual(fs.readdirSync(scenario.directory), []);
  });
}

test("prepare refuses a symlink staged by compose", (context) => {
  const scenario = fixture(context);
  scenario.copy = (source, destination) => {
    if (source === SALT_SOURCE)
      fs.symlinkSync(scenario.root, destination, "junction");
    else fs.writeFileSync(destination, CA);
  };
  assert.throws(scenario.prepare, /symlink|regular file/i);
  assert.deepEqual(fs.readdirSync(scenario.directory), []);
});

for (const targetName of ["caPath", "saltPath"]) {
  test(`prepare preflights a different existing ${targetName} before creating either file`, (context) => {
    const scenario = fixture(context);
    const foreign = Buffer.from("DO-NOT-LEAK existing trust");
    fs.writeFileSync(scenario[targetName], foreign);
    const before = fs.readdirSync(scenario.directory);
    assert.throws(scenario.prepare, (error) => {
      assert.ok(error.message.includes(scenario[targetName]));
      assert.match(error.message, /different|differs/i);
      assert.doesNotMatch(
        inspect(error),
        /DO-NOT-LEAK|BEGIN CERTIFICATE|nonsecret-legacy/,
      );
      return true;
    });
    assert.deepEqual(fs.readdirSync(scenario.directory), before);
    assert.deepEqual(fs.readFileSync(scenario[targetName]), foreign);
  });
}

for (const kind of ["directory", "symlink"]) {
  test(`prepare refuses a ${kind} target before creating the other file`, (context) => {
    const scenario = fixture(context);
    if (kind === "directory") fs.mkdirSync(scenario.saltPath);
    else fs.symlinkSync(scenario.root, scenario.saltPath, "junction");
    assert.throws(scenario.prepare, /symlink|regular file/i);
    assert.equal(fs.existsSync(scenario.caPath), false);
    assert.equal(
      fs.lstatSync(scenario.saltPath).isSymbolicLink(),
      kind === "symlink",
    );
  });
}

test("prepare refuses a config directory that is a symlink or regular file", (context) => {
  const scenario = fixture(context);
  fs.rmdirSync(scenario.directory);
  fs.symlinkSync(scenario.root, scenario.directory, "junction");
  assert.throws(scenario.prepare, /symlink|directory/i);
  assert.equal(fs.existsSync(join(scenario.root, "bridge.salt")), false);
  fs.unlinkSync(scenario.directory);
  fs.writeFileSync(scenario.directory, "not a directory");
  assert.throws(scenario.prepare, /directory/i);
  assert.equal(fs.readFileSync(scenario.directory, "utf8"), "not a directory");
});

test("prepare reruns leave identical files and their permissions/timestamps untouched", (context) => {
  const scenario = fixture(context);
  scenario.prepare();
  const before = new Map();
  for (const target of [scenario.caPath, scenario.saltPath]) {
    if (process.platform !== "win32") fs.chmodSync(target, 0o640);
    fs.utimesSync(target, new Date(1000000), new Date(1000000));
    before.set(target, fs.statSync(target));
  }
  scenario.prepare();
  for (const target of [scenario.caPath, scenario.saltPath]) {
    const after = fs.statSync(target);
    for (const property of ["ino", "mode", "mtimeMs", "ctimeMs"]) {
      assert.equal(after[property], before.get(target)[property], property);
    }
  }
  assert.equal(scenario.calls.length, 4);
});

test("prepare fills a missing partner without rewriting identical existing trust", (context) => {
  const scenario = fixture(context);
  fs.writeFileSync(scenario.saltPath, SALT);
  const before = fs.statSync(scenario.saltPath);
  scenario.prepare();
  assert.deepEqual(fs.readFileSync(scenario.caPath), CA);
  assert.equal(fs.statSync(scenario.saltPath).ctimeMs, before.ctimeMs);
});

for (const targetName of ["caPath", "saltPath"]) {
  for (const identical of [true, false]) {
    test(`prepare handles an ${identical ? "identical" : "different"} EEXIST race on ${targetName} without overwriting or deleting`, (context) => {
      const scenario = fixture(context);
      const expected = targetName === "caPath" ? CA : SALT;
      const raced = identical
        ? expected
        : Buffer.from("DO-NOT-LEAK raced trust");
      const originalOpen = fs.openSync;
      let intercepted = false;
      context.mock.method(fs, "openSync", (file, flags, ...rest) => {
        if (file === scenario[targetName] && flags === "wx" && !intercepted) {
          intercepted = true;
          fs.writeFileSync(file, raced);
        }
        return originalOpen(file, flags, ...rest);
      });
      syncBuiltinESMExports();
      context.after(() => {
        context.mock.restoreAll();
        syncBuiltinESMExports();
      });
      if (identical) scenario.prepare();
      else assert.throws(scenario.prepare, /different|differs/i);
      assert.equal(intercepted, true, "exclusive creation exercises the race");
      assert.deepEqual(fs.readFileSync(scenario[targetName]), raced);
      if (identical || targetName === "saltPath") {
        assert.deepEqual(fs.readFileSync(scenario.caPath), CA);
      } else {
        assert.equal(fs.existsSync(scenario.saltPath), false);
      }
    });
  }
}

test("prepare CLI rejects missing, extra, and relative arguments without running compose", () => {
  const script = fileURLToPath(
    new URL("./ackplane-compose.mjs", import.meta.url),
  );
  for (const args of [[], [tmpdir(), "extra"], ["relative"]]) {
    const result = spawnSync(process.execPath, [script, "prepare", ...args], {
      encoding: "utf8",
      env: { ...process.env, MINDLEAK_COMPOSE_BIN: "must-not-be-executed" },
    });
    assert.equal(result.status, 1);
    assert.equal(result.stdout, "");
    assert.match(
      result.stderr,
      /ackplane-compose: .*absolute|prepare ABSOLUTE_CONFIG_DIR/i,
    );
    assert.doesNotMatch(
      result.stderr,
      /must-not-be-executed|ENOENT|BEGIN CERTIFICATE/,
    );
  }
});
