import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { main } from "./migration-audit.mjs";

// Hardcoded Docker skipped healthy Podman databases and omitted their applied keys.
test("an explicit Podman audit reads both databases instead of using Docker", async (context) => {
  const output = [];
  const calls = [];
  context.mock.method(console, "log", (value) => output.push(value));

  await main(["--next", "--engine", "podman", "--container", "fixture"], {
    env: {},
    execute(command, args, options) {
      calls.push({ command, args, options });
      return "9000|applied-digest\n";
    },
  });

  assert.deepEqual(
    calls,
    ["ackplane", "ackplane_test"].map((database) => ({
      command: "podman",
      args: [
        "exec",
        "fixture",
        "psql",
        "-U",
        "ackplane",
        "-d",
        database,
        "-t",
        "-A",
        "-c",
        "SELECT migration_key, coalesce(content_digest, '') FROM ackplane_schema_migrations ORDER BY migration_key",
      ],
      options: { encoding: "utf8" },
    })),
  );
  assert.deepEqual(output, [9001]);
});

for (const scenario of [
  {
    name: "an explicit engine takes precedence over the configured engine",
    argv: ["--engine", "podman"],
    env: { MINDLEAK_COMPOSE_BIN: "docker" },
    expected: "podman",
  },
  {
    name: "the audit honors the existing Compose engine setting",
    argv: [],
    env: { MINDLEAK_COMPOSE_BIN: "podman" },
    expected: "podman",
  },
  {
    name: "an unconfigured audit retains the Docker default",
    argv: [],
    env: {},
    expected: "docker",
  },
  {
    name: "an empty configured engine retains the Docker default",
    argv: [],
    env: { MINDLEAK_COMPOSE_BIN: "" },
    expected: "docker",
  },
  {
    name: "engine paths with spaces and metacharacters remain one executable",
    argv: ["--engine", "/tools with spaces/podman;literal"],
    env: {},
    expected: "/tools with spaces/podman;literal",
  },
]) {
  test(scenario.name, async (context) => {
    const calls = [];
    context.mock.method(console, "log", () => {});
    await main(["--next", ...scenario.argv], {
      env: {
        ...scenario.env,
        ACKPLANE_POSTGRES_CONTAINER: "configured-container",
      },
      execute(command, args, options) {
        calls.push({ command, args, options });
        return "9000|applied-digest\n";
      },
    });
    assert.equal(calls.length, 2);
    for (const call of calls) {
      assert.equal(call.command, scenario.expected);
      assert.deepEqual(call.args.slice(0, 3), [
        "exec",
        "configured-container",
        "psql",
      ]);
      assert.equal(call.options.shell, undefined);
    }
  });
}

test("a missing engine value refuses before any external command runs", async () => {
  for (const argv of [
    ["--engine"],
    ["--engine", "--next"],
    ["--engine", " "],
  ]) {
    const calls = [];
    await assert.rejects(
      main(argv, {
        env: { MINDLEAK_COMPOSE_BIN: "podman" },
        execute(...args) {
          calls.push(args);
          return "";
        },
      }),
      /--engine requires a container engine executable/,
    );
    assert.deepEqual(calls, []);
  }
});

test("the real CLI reports malformed engine input with exit two", () => {
  const result = spawnSync(
    process.execPath,
    [
      fileURLToPath(new URL("./migration-audit.mjs", import.meta.url)),
      "--engine",
    ],
    { encoding: "utf8" },
  );
  assert.equal(result.status, 2);
  assert.equal(result.stdout, "");
  assert.equal(
    result.stderr,
    "--engine requires a container engine executable\n",
  );
});

test("an unavailable selected engine reports source-only evidence without trying another engine", async (context) => {
  const output = [];
  const errors = [];
  const calls = [];
  context.mock.method(console, "log", (value) => output.push(value));
  context.mock.method(console, "error", (value) => errors.push(value));

  await main(["--next", "--engine", "podman"], {
    env: {},
    execute(command) {
      calls.push(command);
      throw new Error("unavailable");
    },
  });

  assert.deepEqual(calls, ["podman", "podman"]);
  assert.equal(output.length, 1);
  assert.ok(Number.isInteger(output[0]) && output[0] > 0);
  assert.deepEqual(errors, [
    '(no live database reachable via podman container "ackplane-postgres-1"; based on committed source only)',
  ]);
});

test("the test database still contributes its applied keys when the service database is unavailable", async (context) => {
  const output = [];
  const errors = [];
  context.mock.method(console, "log", (value) => output.push(value));
  context.mock.method(console, "error", (value) => errors.push(value));

  await main(["--next", "--engine", "podman"], {
    env: {},
    execute(command, args) {
      assert.equal(command, "podman");
      if (args[args.indexOf("-d") + 1] === "ackplane") {
        throw new Error("service database unavailable");
      }
      return "9000|applied-digest\n";
    },
  });

  assert.deepEqual(output, [9001]);
  assert.deepEqual(errors, []);
});

test("check still reports live digest mismatches without treating them as static failures", async (context) => {
  const output = [];
  const calls = [];
  const exitCode = process.exitCode;
  context.mock.method(console, "log", (value) => output.push(value));

  try {
    await main(["--check", "--engine", "podman"], {
      env: {},
      execute(command, args) {
        calls.push(command);
        if (command === "git") {
          assert.deepEqual(args, [
            "diff",
            "--name-status",
            "origin/main",
            "--",
            "crates/ackplane-server/migrations",
          ]);
          return "";
        }
        return "1|outdated-digest\n";
      },
    });

    assert.deepEqual(calls, ["podman", "podman", "git"]);
    assert.match(output.join("\n"), /ackplane: key 1/);
    assert.match(output.join("\n"), /ackplane_test: key 1/);
    assert.match(output.at(-1), /2 key\(s\) applied under different content/);
    assert.equal(process.exitCode, exitCode);
  } finally {
    process.exitCode = exitCode;
  }
});
