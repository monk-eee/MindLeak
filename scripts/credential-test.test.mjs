import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, statSync } from "node:fs";
import { resolve } from "node:path";

import { runCredentialTest } from "./credential-test.mjs";

test("a missing command refuses before launching any process", () => {
  assert.equal(
    runCredentialTest({
      run: () => assert.fail("must not launch"),
      error: () => {},
    }),
    2,
  );
});

test("native platforms require credentials and preserve literal command arguments", () => {
  for (const platform of ["darwin", "win32"]) {
    const env = { MINDLEAK_REQUIRE_CREDENTIAL_FACILITY: "0" };
    const code = runCredentialTest({
      command: "cargo",
      args: ["test", "argument with spaces"],
      platform,
      env,
      run: (command, args, options) => {
        assert.equal(command, "cargo");
        assert.deepEqual(args, ["test", "argument with spaces"]);
        assert.equal(options.env.MINDLEAK_REQUIRE_CREDENTIAL_FACILITY, "1");
        assert.equal(options.stdio, "inherit");
        assert.equal(options.shell, undefined);
        return { status: 0 };
      },
    });
    assert.equal(code, 0);
    assert.equal(env.MINDLEAK_REQUIRE_CREDENTIAL_FACILITY, "0");
  }
});

test("Linux starts a fresh bus with private temporary keyring directories and removes them", () => {
  let directory;
  const code = runCredentialTest({
    command: "cargo",
    args: ["test"],
    platform: "linux",
    env: {
      DBUS_SESSION_BUS_ADDRESS: "live-bus",
      GNOME_KEYRING_CONTROL: "live-keyring",
    },
    run: (command, args, options) => {
      assert.equal(command, "dbus-run-session");
      assert.deepEqual(args.slice(-4), [
        "--inside-session",
        "--",
        "cargo",
        "test",
      ]);
      assert.equal(options.env.DBUS_SESSION_BUS_ADDRESS, undefined);
      assert.equal(options.env.GNOME_KEYRING_CONTROL, undefined);
      directory = options.env.MINDLEAK_CREDENTIAL_TEST_SESSION;
      assert.ok(existsSync(directory));
      for (const name of [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_RUNTIME_DIR",
      ]) {
        assert.ok(options.env[name].startsWith(directory));
        if (process.platform !== "win32")
          assert.equal(statSync(options.env[name]).mode & 0o777, 0o700);
      }
      return { status: 17 };
    },
    error: () => {},
  });
  assert.equal(code, 17);
  assert.equal(existsSync(directory), false);
});

test("a missing isolated bus refuses before keyring access", () => {
  assert.equal(
    runCredentialTest({
      command: "cargo",
      platform: "linux",
      insideSession: true,
      env: {},
      run: () => assert.fail("must not use live keyring"),
      error: () => {},
    }),
    2,
  );
});

test("test keyring starts before the command without evaluating its output", () => {
  const calls = [];
  const code = runCredentialTest({
    command: "cargo",
    args: ["test"],
    platform: "linux",
    insideSession: true,
    env: {
      DBUS_SESSION_BUS_ADDRESS: "isolated-bus",
      MINDLEAK_CREDENTIAL_TEST_SESSION: resolve("isolated-test"),
    },
    run: (command, args, options) => {
      calls.push(command);
      if (command === "gnome-keyring-daemon") {
        assert.ok(args.includes("--daemonize"));
        assert.equal(options.input, "\n");
        assert.equal(options.timeout, 10000);
        return { status: 0, stdout: "do-not-evaluate-this-output" };
      }
      assert.deepEqual(args, ["test"]);
      assert.equal(options.env.MINDLEAK_REQUIRE_CREDENTIAL_FACILITY, "1");
      return { status: 0 };
    },
  });
  assert.equal(code, 0);
  assert.deepEqual(calls, ["gnome-keyring-daemon", "cargo"]);
});

test("startup failure or termination never runs tests and never echoes process output", () => {
  for (const result of [
    { status: 7, stderr: "private-data" },
    { status: null, signal: "SIGTERM" },
    { status: null, error: { code: "ENOENT", message: "private-data" } },
  ]) {
    let calls = 0;
    const errors = [];
    const code = runCredentialTest({
      command: "cargo",
      platform: "linux",
      insideSession: true,
      env: {
        DBUS_SESSION_BUS_ADDRESS: "isolated-bus",
        MINDLEAK_CREDENTIAL_TEST_SESSION: resolve("isolated-test"),
      },
      run: () => {
        calls += 1;
        return result;
      },
      error: (message) => errors.push(message),
    });
    assert.equal(calls, 1);
    assert.notEqual(code, 0);
    assert.doesNotMatch(errors.join("\n"), /private-data/);
  }
});
