import assert from "node:assert/strict";
import { test } from "node:test";

import {
  configuredAttempts,
  configuredDelayMs,
  isTransientNpmFailure,
  runNpmCi,
} from "./npm-ci-retry.mjs";

const result = (status, stderr = "") => ({ status, stdout: "", stderr });

test("recognizes only known transient npm network failures", () => {
  assert.equal(isTransientNpmFailure(result(1, "npm error code E503")), true);
  assert.equal(
    isTransientNpmFailure(result(1, "npm error code ECONNRESET")),
    true,
  );
  assert.equal(
    isTransientNpmFailure(result(1, "npm error code ELOCKVERIFY")),
    false,
  );
  assert.equal(isTransientNpmFailure(result(0, "npm error code E503")), false);
});

test("restarts npm ci once after a transient registry failure", () => {
  const outcomes = [result(1, "npm error code E503"), result(0, "installed")];
  const calls = [];
  const waits = [];
  const output = [];
  const exit = runNpmCi({
    npmArgs: ["--prefix", "editors/vscode"],
    attempts: 2,
    delayMs: 25,
    run: (args) => {
      calls.push(args);
      return outcomes.shift();
    },
    wait: (milliseconds) => waits.push(milliseconds),
    write: (_stream, text) => output.push(text),
  });

  assert.equal(exit, 0);
  assert.deepEqual(calls, [
    ["--prefix", "editors/vscode", "ci"],
    ["--prefix", "editors/vscode", "ci"],
  ]);
  assert.deepEqual(waits, [25]);
  assert.match(output.join(""), /retrying npm ci \(2\/2\)/);
});

test("fails immediately for a deterministic npm error", () => {
  const calls = [];
  const waits = [];
  const exit = runNpmCi({
    attempts: 2,
    run: (args) => {
      calls.push(args);
      return result(1, "npm error code ELOCKVERIFY");
    },
    wait: (milliseconds) => waits.push(milliseconds),
    write: () => {},
  });

  assert.equal(exit, 1);
  assert.deepEqual(calls, [["ci"]]);
  assert.deepEqual(waits, []);
});

test("stops after the configured number of transient failures", () => {
  const waits = [];
  const exit = runNpmCi({
    attempts: 2,
    delayMs: 50,
    run: () => result(1, "npm error code ETIMEDOUT"),
    wait: (milliseconds) => waits.push(milliseconds),
    write: () => {},
  });

  assert.equal(exit, 1);
  assert.deepEqual(waits, [50]);
});

test("bounds malformed retry configuration to safe defaults", () => {
  assert.equal(configuredAttempts("3"), 3);
  assert.equal(configuredAttempts("4"), 2);
  assert.equal(configuredAttempts("nope"), 2);
  assert.equal(configuredDelayMs("1000"), 1000);
  assert.equal(configuredDelayMs("120001"), 30_000);
  assert.equal(configuredDelayMs("nope"), 30_000);
});
