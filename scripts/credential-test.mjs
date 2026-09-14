import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { isAbsolute, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const script = fileURLToPath(import.meta.url);
const sessionVariable = "MINDLEAK_CREDENTIAL_TEST_SESSION";

function launch(command, args, options, run) {
  try {
    const child = run(command, args, options);
    const process = { child, finished: false };
    process.done = new Promise((resolve) => {
      const finish = (result) => {
        process.finished = true;
        resolve(result);
      };
      child.once("error", (error) => finish({ error }));
      child.once("exit", (status, signal) => finish({ status, signal }));
    });
    return process;
  } catch (error) {
    return { finished: true, done: Promise.resolve({ error }) };
  }
}

async function bounded(promise, timeoutMs) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((resolve) => {
        timer = setTimeout(
          () => resolve({ error: { code: "ETIMEDOUT" } }),
          timeoutMs,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

function outcome(result, label, error) {
  if (!result.error && !result.signal && result.status === 0) return 0;
  const reason =
    result.error?.code ?? result.signal ?? `exit ${result.status ?? "unknown"}`;
  error(`credential-test: ${label} failed (${reason})`);
  return Number.isInteger(result.status) && result.status > 0
    ? result.status
    : 1;
}

export async function runCredentialTest({
  command,
  args = [],
  env = process.env,
  platform = process.platform,
  insideSession = false,
  run = spawn,
  terminate = (child, signal) => {
    if (child.pid !== undefined) process.kill(-child.pid, signal);
  },
  signals = process,
  startupTimeoutMs = 10_000,
  shutdownTimeoutMs = 1_000,
  error = console.error,
} = {}) {
  if (!command) {
    error("credential-test: specify a command after --");
    return 2;
  }
  const environment = { ...env, MINDLEAK_REQUIRE_CREDENTIAL_FACILITY: "1" };
  if (platform !== "linux") {
    return outcome(
      await launch(command, args, { env: environment, stdio: "inherit" }, run)
        .done,
      "test command",
      error,
    );
  }
  if (!insideSession) {
    const directory = mkdtempSync(join(tmpdir(), "mindleak-credential-test-"));
    try {
      for (const child of ["config", "data", "run", "control"]) {
        mkdirSync(join(directory, child), { mode: 0o700 });
      }
      environment[sessionVariable] = directory;
      environment.XDG_CONFIG_HOME = join(directory, "config");
      environment.XDG_DATA_HOME = join(directory, "data");
      environment.XDG_RUNTIME_DIR = join(directory, "run");
      delete environment.DBUS_SESSION_BUS_ADDRESS;
      delete environment.GNOME_KEYRING_CONTROL;
      delete environment.GNOME_KEYRING_PID;
      return outcome(
        await launch(
          "dbus-run-session",
          [
            "--",
            process.execPath,
            script,
            "--inside-session",
            "--",
            command,
            ...args,
          ],
          {
            env: environment,
            stdio: "inherit",
          },
          run,
        ).done,
        "isolated credential session",
        error,
      );
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  }
  const directory = environment[sessionVariable];
  if (
    !directory ||
    !isAbsolute(directory) ||
    !environment.DBUS_SESSION_BUS_ADDRESS
  ) {
    error("credential-test: an isolated D-Bus session is required");
    return 2;
  }
  environment.GNOME_KEYRING_CONTROL = join(directory, "control");
  const keyring = launch(
    "gnome-keyring-daemon",
    [
      "--unlock",
      "--components=secrets",
      "--foreground",
      "--control-directory",
      join(directory, "control"),
    ],
    {
      env: environment,
      stdio: ["pipe", "pipe", "pipe"],
      detached: true,
    },
    run,
  );
  let interrupted;
  const interruption = new Promise((resolve) => {
    interrupted = resolve;
  });
  const interrupt = () => interrupted({ interrupted: 130 });
  const terminateSession = () => interrupted({ interrupted: 143 });
  signals.on("SIGINT", interrupt);
  signals.on("SIGTERM", terminateSession);
  const stop = async (owned, label) => {
    if (!owned?.child) return true;
    try {
      terminate(owned.child, "SIGTERM");
      if (!owned.finished) {
        await bounded(owned.done, shutdownTimeoutMs);
      }
      terminate(owned.child, "SIGKILL");
      if (!owned.finished) {
        const killed = await bounded(owned.done, shutdownTimeoutMs);
        if (killed.error?.code === "ETIMEDOUT") {
          error(`credential-test: ${label} did not exit after SIGKILL`);
          return false;
        }
      }
    } catch (failure) {
      if (failure.code !== "ESRCH") {
        error(
          `credential-test: ${label} cleanup failed (${failure.code ?? "unknown"})`,
        );
        return false;
      }
    }
    return true;
  };
  let tests;
  let code = 1;
  try {
    const ready = new Promise((resolve) => {
      keyring.child?.stdout.once("end", () => resolve({ ready: true }));
    });
    const inputFailed = new Promise((resolve) => {
      keyring.child?.stdin.once("error", (error) => resolve({ error }));
    });
    keyring.child?.stdout.resume();
    keyring.child?.stderr.resume();
    keyring.child?.stdin.end("\n");
    const started = await bounded(
      Promise.race([ready, keyring.done, inputFailed, interruption]),
      startupTimeoutMs,
    );
    if (started.interrupted) {
      code = started.interrupted;
    } else if (!started.ready || keyring.finished) {
      code = outcome(
        started.ready ? await keyring.done : started,
        "test keyring startup",
        error,
      );
      if (code === 0) {
        error("credential-test: test keyring exited before readiness (exit 0)");
        code = 1;
      }
    } else {
      tests = launch(
        command,
        args,
        { env: environment, stdio: "inherit", detached: true },
        run,
      );
      const result = await Promise.race([
        tests.done.then((result) => ({ ...result, origin: "tests" })),
        keyring.done.then((result) => ({ ...result, origin: "keyring" })),
        interruption,
      ]);
      code =
        result.interrupted ??
        outcome(
          result,
          result.origin === "keyring"
            ? "test keyring exited during tests"
            : "test command",
          error,
        );
      if (result.origin === "keyring" && code === 0) {
        error("credential-test: test keyring exited during tests (exit 0)");
        code = 1;
      }
    }
  } finally {
    const testsStopped = await stop(tests, "test process group");
    const keyringStopped = await stop(keyring, "test keyring");
    if (!testsStopped || !keyringStopped) code ||= 1;
    signals.off("SIGINT", interrupt);
    signals.off("SIGTERM", terminateSession);
  }
  return code;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const args = process.argv.slice(2);
  const insideSession = args[0] === "--inside-session";
  if (insideSession) args.shift();
  if (args.shift() !== "--") {
    console.error(
      "Usage: node scripts/credential-test.mjs -- <command> [args...]",
    );
    process.exitCode = 2;
  } else {
    process.exitCode = await runCredentialTest({
      command: args.shift(),
      args,
      insideSession,
    });
  }
}
