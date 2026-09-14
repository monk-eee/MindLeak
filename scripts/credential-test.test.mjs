import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { existsSync, statSync } from "node:fs";
import { resolve } from "node:path";
import { PassThrough } from "node:stream";

import { runCredentialTest } from "./credential-test.mjs";

test("a lost Linux credential daemon fails the run and stops its test process", async () => {
  const processes = [];
  const terminated = [];
  const errors = [];
  const result = await runCredentialTest({
    command: "cargo",
    args: ["test"],
    platform: "linux",
    insideSession: true,
    env: {
      DBUS_SESSION_BUS_ADDRESS: "isolated-bus",
      MINDLEAK_CREDENTIAL_TEST_SESSION: resolve("isolated-test"),
    },
    run: (command) => {
      const child = fakeProcess({
        ready: command === "gnome-keyring-daemon",
        pending: true,
      });
      child.command = command;
      processes.push(child);
      if (command !== "gnome-keyring-daemon") {
        queueMicrotask(() => {
          processes[0].finish(null, "SIGKILL");
        });
      }
      return child;
    },
    terminate: (child, signal) => {
      terminated.push({ command: child.command, signal });
      child.kill(signal);
    },
    error: (message) => errors.push(message),
  });

  // Daemon loss used to leave credential calls blocked until the whole CI job timed out.
  assert.notEqual(result, 0);
  assert.match(errors.join("\n"), /keyring.*during.*SIGKILL/);
  assert.ok(terminated.some(({ command }) => command === "cargo"));
  assert.ok(
    terminated.some(
      ({ command, signal }) => command === "cargo" && signal === "SIGKILL",
    ),
  );
});

function fakeProcess(result = { status: 0 }) {
  const child = new EventEmitter();
  child.exitCode = null;
  child.signalCode = null;
  child.stdin = new PassThrough();
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  child.finish = (status, signal) => {
    child.exitCode = status;
    child.signalCode = signal;
    child.emit("exit", status, signal);
  };
  child.kill = (signal) => {
    queueMicrotask(() => child.finish(null, signal));
    return true;
  };
  queueMicrotask(() => {
    if (result.ready) child.stdout.end();
    else if (result.error) child.emit("error", result.error);
    else if (!result.pending)
      child.finish(result.status ?? null, result.signal ?? null);
  });
  return child;
}

test("a missing command refuses before launching any process", async () => {
  assert.equal(
    await runCredentialTest({
      run: () => assert.fail("must not launch"),
      error: () => {},
    }),
    2,
  );
});

test("native platforms require credentials and preserve literal command arguments", async () => {
  for (const platform of ["darwin", "win32"]) {
    const env = { MINDLEAK_REQUIRE_CREDENTIAL_FACILITY: "0" };
    const code = await runCredentialTest({
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
        return fakeProcess();
      },
    });
    assert.equal(code, 0);
    assert.equal(env.MINDLEAK_REQUIRE_CREDENTIAL_FACILITY, "0");
  }
});

test("Linux starts a fresh bus with private temporary keyring directories and removes them", async () => {
  let directory;
  const code = await runCredentialTest({
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
      return fakeProcess({ status: 17 });
    },
    error: () => {},
  });
  assert.equal(code, 17);
  assert.equal(existsSync(directory), false);
});

test("a missing isolated bus refuses before keyring access", async () => {
  assert.equal(
    await runCredentialTest({
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

test("test keyring starts before the command without evaluating its output", async () => {
  const calls = [];
  const code = await runCredentialTest({
    command: "cargo",
    args: ["test"],
    platform: "linux",
    insideSession: true,
    env: {
      DBUS_SESSION_BUS_ADDRESS: "isolated-bus",
      MINDLEAK_CREDENTIAL_TEST_SESSION: resolve("isolated-test"),
    },
    terminate: (child, signal) => child.kill(signal),
    run: (command, args, options) => {
      calls.push(command);
      if (command === "gnome-keyring-daemon") {
        assert.ok(args.includes("--foreground"));
        assert.equal(options.detached, true);
        const child = fakeProcess({ ready: true });
        child.stdout.write("do-not-evaluate-this-output");
        return child;
      }
      assert.deepEqual(args, ["test"]);
      assert.equal(options.env.MINDLEAK_REQUIRE_CREDENTIAL_FACILITY, "1");
      assert.equal(
        options.env.GNOME_KEYRING_CONTROL,
        resolve("isolated-test/control"),
      );
      return fakeProcess();
    },
  });
  assert.equal(code, 0);
  assert.deepEqual(calls, ["gnome-keyring-daemon", "cargo"]);
});

test("startup failure or termination never runs tests and never echoes process output", async () => {
  for (const result of [
    { status: 0 },
    { status: 7, stderr: "private-data" },
    { status: null, signal: "SIGTERM" },
    { status: null, error: { code: "ENOENT", message: "private-data" } },
  ]) {
    let calls = 0;
    const errors = [];
    const code = await runCredentialTest({
      command: "cargo",
      platform: "linux",
      insideSession: true,
      env: {
        DBUS_SESSION_BUS_ADDRESS: "isolated-bus",
        MINDLEAK_CREDENTIAL_TEST_SESSION: resolve("isolated-test"),
      },
      terminate: (child, signal) => child.kill(signal),
      run: () => {
        calls += 1;
        return fakeProcess(result);
      },
      error: (message) => errors.push(message),
    });
    assert.equal(calls, 1);
    assert.notEqual(code, 0);
    assert.ok(errors.length > 0);
    assert.doesNotMatch(errors.join("\n"), /private-data/);
  }
});

test("an unready daemon times out before any test starts and is stopped", async () => {
  const calls = [];
  const stopped = [];
  const errors = [];
  const code = await runCredentialTest({
    command: "cargo",
    platform: "linux",
    insideSession: true,
    startupTimeoutMs: 5,
    env: {
      DBUS_SESSION_BUS_ADDRESS: "isolated-bus",
      MINDLEAK_CREDENTIAL_TEST_SESSION: resolve("isolated-test"),
    },
    run: (command) => {
      calls.push(command);
      return fakeProcess({ pending: true });
    },
    terminate: (child, signal) => {
      stopped.push(signal);
      child.kill(signal);
    },
    error: (message) => errors.push(message),
  });
  assert.equal(code, 1);
  assert.deepEqual(calls, ["gnome-keyring-daemon"]);
  assert.deepEqual(stopped, ["SIGTERM", "SIGKILL"]);
  assert.match(errors.join("\n"), /startup.*ETIMEDOUT/);
});

test("cleanup escalates an unresponsive owned daemon and preserves the test exit", async () => {
  const stopped = [];
  const code = await runCredentialTest({
    command: "cargo",
    platform: "linux",
    insideSession: true,
    shutdownTimeoutMs: 5,
    env: {
      DBUS_SESSION_BUS_ADDRESS: "isolated-bus",
      MINDLEAK_CREDENTIAL_TEST_SESSION: resolve("isolated-test"),
    },
    run: (command) => {
      const child = fakeProcess(
        command === "gnome-keyring-daemon" ? { ready: true } : { status: 17 },
      );
      child.command = command;
      return child;
    },
    terminate: (child, signal) => {
      stopped.push({ command: child.command, signal });
      if (signal === "SIGKILL") child.kill(signal);
    },
    error: () => {},
  });
  assert.equal(code, 17);
  assert.ok(
    stopped.some(
      ({ command, signal }) =>
        command === "gnome-keyring-daemon" && signal === "SIGKILL",
    ),
  );
});

test("interrupting startup clears its deadline and stops the owned daemon", async () => {
  const signals = new EventEmitter();
  const stopped = [];
  const code = await runCredentialTest({
    command: "cargo",
    platform: "linux",
    insideSession: true,
    signals,
    env: {
      DBUS_SESSION_BUS_ADDRESS: "isolated-bus",
      MINDLEAK_CREDENTIAL_TEST_SESSION: resolve("isolated-test"),
    },
    run: () => {
      const child = fakeProcess({ pending: true });
      queueMicrotask(() => signals.emit("SIGTERM"));
      return child;
    },
    terminate: (child, signal) => {
      stopped.push(signal);
      child.kill(signal);
    },
    error: () => {},
  });
  assert.equal(code, 143);
  assert.deepEqual(stopped, ["SIGTERM", "SIGKILL"]);
  assert.equal(signals.listenerCount("SIGINT"), 0);
  assert.equal(signals.listenerCount("SIGTERM"), 0);
});

test("interrupting tests stops both owned process groups and preserves the signal status", async () => {
  const signals = new EventEmitter();
  const stopped = [];
  const code = await runCredentialTest({
    command: "cargo",
    platform: "linux",
    insideSession: true,
    signals,
    env: {
      DBUS_SESSION_BUS_ADDRESS: "isolated-bus",
      MINDLEAK_CREDENTIAL_TEST_SESSION: resolve("isolated-test"),
    },
    run: (command) => {
      const child = fakeProcess({
        ready: command === "gnome-keyring-daemon",
        pending: true,
      });
      child.command = command;
      if (command === "cargo") queueMicrotask(() => signals.emit("SIGINT"));
      return child;
    },
    terminate: (child, signal) => {
      stopped.push({ command: child.command, signal });
      child.kill(signal);
    },
    error: () => {},
  });
  assert.equal(code, 130);
  assert.deepEqual(stopped, [
    { command: "cargo", signal: "SIGTERM" },
    { command: "cargo", signal: "SIGKILL" },
    { command: "gnome-keyring-daemon", signal: "SIGTERM" },
    { command: "gnome-keyring-daemon", signal: "SIGKILL" },
  ]);
});

test("failed daemon cleanup cannot turn successful tests into a passing run", async () => {
  const errors = [];
  const code = await runCredentialTest({
    command: "cargo",
    platform: "linux",
    insideSession: true,
    shutdownTimeoutMs: 5,
    env: {
      DBUS_SESSION_BUS_ADDRESS: "isolated-bus",
      MINDLEAK_CREDENTIAL_TEST_SESSION: resolve("isolated-test"),
    },
    run: (command) =>
      fakeProcess(
        command === "gnome-keyring-daemon" ? { ready: true } : { status: 0 },
      ),
    terminate: () => {},
    error: (message) => errors.push(message),
  });
  assert.equal(code, 1);
  assert.match(errors.join("\n"), /did not exit after SIGKILL/);
});
