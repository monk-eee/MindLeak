// Retry a complete npm ci process only when the package registry failed
// transiently. npm's fetch retry loop helps individual requests, but a proxy
// outage can outlast it and leave the whole installation half-written.
//
// Platform-agnostic: resolve npm's JavaScript CLI from Node's own installation.
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";

export const DEFAULT_ATTEMPTS = 2;
export const DEFAULT_DELAY_MS = 30_000;
const MAX_ATTEMPTS = 3;
const MAX_DELAY_MS = 120_000;
const TRANSIENT_NPM_FAILURE =
  /\b(?:E503|EAI_AGAIN|ECONNRESET|ECONNREFUSED|EHOSTUNREACH|ENETUNREACH|ETIMEDOUT)\b/;

export function configuredAttempts(value) {
  const parsed = Number.parseInt(value ?? "", 10);
  return Number.isInteger(parsed) && parsed >= 1 && parsed <= MAX_ATTEMPTS
    ? parsed
    : DEFAULT_ATTEMPTS;
}

export function configuredDelayMs(value) {
  const parsed = Number.parseInt(value ?? "", 10);
  return Number.isInteger(parsed) && parsed >= 1_000 && parsed <= MAX_DELAY_MS
    ? parsed
    : DEFAULT_DELAY_MS;
}

export function isTransientNpmFailure(result) {
  if (result.status === 0) return false;
  const output = `${result.stdout ?? ""}\n${result.stderr ?? ""}`;
  return TRANSIENT_NPM_FAILURE.test(output);
}

export function npmExitCode(result) {
  return typeof result.status === "number" ? result.status : 1;
}

export function npmCliPath(nodeExecutable = process.execPath) {
  return join(
    dirname(nodeExecutable),
    "node_modules",
    "npm",
    "bin",
    "npm-cli.js",
  );
}

export function runNpmCi({
  npmArgs = [],
  attempts = DEFAULT_ATTEMPTS,
  delayMs = DEFAULT_DELAY_MS,
  run = (args) =>
    spawnSync(process.execPath, [npmCliPath(), ...args], {
      encoding: "utf8",
      maxBuffer: 4 * 1024 * 1024,
      // Calling npm's JavaScript entrypoint avoids Windows .cmd shim behavior
      // and keeps stdout/stderr available for retry classification.
    }),
  wait = (milliseconds) =>
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, milliseconds),
  write = (stream, text) => {
    if (text) stream.write(text);
  },
}) {
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    const result = run([...npmArgs, "ci"]);
    write(process.stdout, result.stdout);
    write(process.stderr, result.stderr);
    if (result.error) {
      write(
        process.stderr,
        `npm-ci-retry: could not start npm: ${result.error.message}\n`,
      );
      return 1;
    }
    if (result.status === 0) return 0;

    if (attempt === attempts || !isTransientNpmFailure(result)) {
      return npmExitCode(result);
    }

    write(
      process.stderr,
      `npm-ci-retry: transient registry failure; retrying npm ci (${attempt + 1}/${attempts}) after ${delayMs}ms\n`,
    );
    wait(delayMs);
  }
  return 1;
}

if (
  process.argv[1] &&
  import.meta.url.endsWith(process.argv[1].replace(/\\/g, "/"))
) {
  process.exitCode = runNpmCi({
    npmArgs: process.argv.slice(2),
    attempts: configuredAttempts(process.env.NPM_CI_RETRY_ATTEMPTS),
    delayMs: configuredDelayMs(process.env.NPM_CI_RETRY_DELAY_MS),
  });
}
