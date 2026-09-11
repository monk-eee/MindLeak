import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { isAbsolute, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const script = fileURLToPath(import.meta.url);
const sessionVariable = "MINDLEAK_CREDENTIAL_TEST_SESSION";

function outcome(result, label, error) {
  if (!result.error && !result.signal && result.status === 0) return 0;
  const reason =
    result.error?.code ?? result.signal ?? `exit ${result.status ?? "unknown"}`;
  error(`credential-test: ${label} failed (${reason})`);
  return Number.isInteger(result.status) && result.status > 0
    ? result.status
    : 1;
}

export function runCredentialTest({
  command,
  args = [],
  env = process.env,
  platform = process.platform,
  insideSession = false,
  run = spawnSync,
  error = console.error,
} = {}) {
  if (!command) {
    error("credential-test: specify a command after --");
    return 2;
  }
  const environment = { ...env, MINDLEAK_REQUIRE_CREDENTIAL_FACILITY: "1" };
  if (platform !== "linux") {
    return outcome(
      run(command, args, { env: environment, stdio: "inherit" }),
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
        run(
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
        ),
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
  const keyring = run(
    "gnome-keyring-daemon",
    [
      "--unlock",
      "--components=secrets",
      "--daemonize",
      "--control-directory",
      join(directory, "control"),
    ],
    {
      env: environment,
      input: "\n",
      encoding: "utf8",
      stdio: ["pipe", "pipe", "pipe"],
      timeout: 10000,
    },
  );
  const started = outcome(keyring, "test keyring startup", error);
  if (started !== 0) return started;
  return outcome(
    run(command, args, { env: environment, stdio: "inherit" }),
    "test command",
    error,
  );
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
    process.exitCode = runCredentialTest({
      command: args.shift(),
      args,
      insideSession,
    });
  }
}
