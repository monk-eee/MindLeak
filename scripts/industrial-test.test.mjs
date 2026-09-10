import test from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";

import { runIndustrialTests } from "./industrial-test.mjs";

const environment = {
  ACKPLANE_TEST_DATABASE_URL:
    "postgresql://review:local@127.0.0.1:15432/review",
  ACKPLANE_TEST_REHEARSAL_DATABASE_URL:
    "postgresql://review:local@127.0.0.1:15432/review",
};

test("industrial validation refuses either missing database gate before running commands", () => {
  for (const missing of Object.keys(environment)) {
    const env = { ...environment, [missing]: " " };
    const errors = [];
    const result = runIndustrialTests({
      env,
      run: () => assert.fail("no command may run without both database gates"),
      report: () => {},
      error: (message) => errors.push(message),
    });
    assert.equal(result, 2);
    assert.match(errors.join("\n"), new RegExp(missing));
  }
});

test("invalid database URLs are refused without exposing their credentials", () => {
  for (const value of [
    "not-a-url-with-secret",
    "https://review:secret@localhost/review",
    "postgresql://review:secret@localhost/",
  ]) {
    const errors = [];
    const result = runIndustrialTests({
      env: { ...environment, ACKPLANE_TEST_DATABASE_URL: value },
      run: () => assert.fail("an invalid database URL must not reach Cargo"),
      report: () => {},
      error: (message) => errors.push(message),
    });
    assert.equal(result, 2);
    assert.doesNotMatch(errors.join("\n"), /secret/);
  }
});

test("the gate checks recovery tools, builds every target, migrates, then tests with both database gates enabled", () => {
  const calls = [];
  const env = {
    ...environment,
    ACKPLANE_DATABASE_URL: "postgresql://live:secret@production/live",
  };
  const result = runIndustrialTests({
    env,
    run: (command, args, options) => {
      calls.push({ command, args, options });
      return { status: 0 };
    },
    report: () => {},
    error: () => assert.fail("successful commands must not report errors"),
  });

  assert.equal(result, 0);
  assert.deepEqual(
    calls.map(({ command }) => command),
    ["pg_dump", "pg_restore", "cargo", "cargo", "cargo"],
  );
  assert.deepEqual(calls[2].args, [
    "test",
    "--workspace",
    "--all-features",
    "--all-targets",
    "--locked",
    "--jobs",
    "2",
    "--no-run",
  ]);
  assert.deepEqual(calls[3].args, [
    "run",
    "--locked",
    "--package",
    "ackplane-server",
    "--bin",
    "migrate",
    "--jobs",
    "2",
  ]);
  assert.deepEqual(calls[4].args, [
    "test",
    "--workspace",
    "--all-features",
    "--locked",
    "--jobs",
    "2",
    "--",
    "--test-threads=4",
  ]);
  for (const { options } of calls) {
    assert.equal(
      options.env.ACKPLANE_DATABASE_URL,
      environment.ACKPLANE_TEST_DATABASE_URL,
    );
    assert.equal(
      options.env.ACKPLANE_TEST_DATABASE_URL,
      environment.ACKPLANE_TEST_DATABASE_URL,
    );
    assert.equal(
      options.env.ACKPLANE_TEST_REHEARSAL_DATABASE_URL,
      environment.ACKPLANE_TEST_REHEARSAL_DATABASE_URL,
    );
    assert.equal(options.stdio, "inherit");
    assert.equal(options.cwd, fileURLToPath(new URL("../", import.meta.url)));
  }
  assert.equal(
    env.ACKPLANE_DATABASE_URL,
    "postgresql://live:secret@production/live",
  );
});

test("a failed migration stops validation and preserves its exit code", () => {
  const calls = [];
  const errors = [];
  const result = runIndustrialTests({
    env: environment,
    run: (command, args) => {
      calls.push({ command, args });
      return { status: args.includes("migrate") ? 17 : 0 };
    },
    report: () => {},
    error: (message) => errors.push(message),
  });
  assert.equal(result, 17);
  assert.equal(calls.length, 4);
  assert.match(errors.join("\n"), /migrat/i);
});

test("a missing recovery executable fails before a build or database write", () => {
  let calls = 0;
  const errors = [];
  const result = runIndustrialTests({
    env: environment,
    run: () => {
      calls += 1;
      return { status: null, error: { code: "ENOENT" } };
    },
    report: () => {},
    error: (message) => errors.push(message),
  });
  assert.equal(result, 1);
  assert.equal(calls, 1);
  assert.match(errors.join("\n"), /pg_dump/);
});

test("a terminated command cannot report a passing gate", () => {
  assert.equal(
    runIndustrialTests({
      env: environment,
      run: () => ({ status: null, signal: "SIGTERM" }),
      report: () => {},
      error: () => {},
    }),
    1,
  );
});
