import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

const workspace = fileURLToPath(new URL("../", import.meta.url));
const databaseVariables = [
  "ACKPLANE_TEST_DATABASE_URL",
  "ACKPLANE_TEST_REHEARSAL_DATABASE_URL",
];

export function runIndustrialTests({
  env = process.env,
  run = spawnSync,
  report = console.log,
  error = console.error,
} = {}) {
  const environment = { ...env };
  for (const variable of databaseVariables) {
    const value = environment[variable]?.trim();
    if (!value) {
      error(
        `industrial-test: ${variable} must name an isolated test database; refusing a skipped database suite`,
      );
      return 2;
    }
    try {
      const url = new URL(value);
      if (
        !["postgres:", "postgresql:"].includes(url.protocol) ||
        !url.hostname ||
        url.pathname.length < 2
      ) {
        throw new Error("invalid database URL");
      }
    } catch {
      error(
        `industrial-test: ${variable} must be a PostgreSQL URL with an explicit database name`,
      );
      return 2;
    }
    environment[variable] = value;
  }
  environment.ACKPLANE_DATABASE_URL = environment.ACKPLANE_TEST_DATABASE_URL;
  environment.MINDLEAK_REQUIRE_CREDENTIAL_FACILITY = "1";

  const steps = [
    { name: "check pg_dump", command: "pg_dump", args: ["--version"] },
    { name: "check pg_restore", command: "pg_restore", args: ["--version"] },
    {
      name: "compile all workspace targets",
      command: "cargo",
      args: [
        "test",
        "--workspace",
        "--all-features",
        "--all-targets",
        "--locked",
        "--jobs",
        "2",
        "--no-run",
      ],
    },
    {
      name: "migrate the test database",
      command: "cargo",
      args: [
        "run",
        "--locked",
        "--package",
        "ackplane-server",
        "--bin",
        "migrate",
        "--jobs",
        "2",
      ],
    },
    {
      name: "test the workspace with database and recovery gates enabled",
      command: process.execPath,
      args: [
        fileURLToPath(new URL("./credential-test.mjs", import.meta.url)),
        "--",
        "cargo",
        "test",
        "--workspace",
        "--all-features",
        "--locked",
        "--jobs",
        "2",
        "--",
        "--test-threads=4",
      ],
    },
  ];
  for (const step of steps) {
    report(`industrial-test: ${step.name}`);
    const result = run(step.command, step.args, {
      cwd: workspace,
      env: environment,
      stdio: "inherit",
    });
    if (result.error || result.signal || result.status !== 0) {
      const reason =
        result.error?.code ??
        result.signal ??
        `exit ${result.status ?? "unknown"}`;
      error(`industrial-test: ${step.name} failed (${reason}); stopping`);
      return Number.isInteger(result.status) && result.status > 0
        ? result.status
        : 1;
    }
  }
  report(
    "industrial-test: passed; database, recovery and native credential test gates were enabled",
  );
  return 0;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const args = process.argv.slice(2);
  if (args.length === 1 && ["--help", "-h"].includes(args[0])) {
    console.log(
      "Usage: node scripts/industrial-test.mjs\nRequires cargo, pg_dump, pg_restore and two explicit PostgreSQL URLs:\n  ACKPLANE_TEST_DATABASE_URL\n  ACKPLANE_TEST_REHEARSAL_DATABASE_URL\nNative credentials are required. Linux also requires dbus-run-session and gnome-keyring-daemon; the runner creates an isolated test session. Use disposable databases only. Migrations and tests write to them; recovery tests create and drop scratch databases. Never point this command at the running deployment.",
    );
  } else if (args.length > 0) {
    console.error("industrial-test: unexpected arguments; use --help");
    process.exitCode = 2;
  } else {
    process.exitCode = runIndustrialTests();
  }
}
