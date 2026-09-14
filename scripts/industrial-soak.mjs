import { performance } from "node:perf_hooks";
import { spawnSync } from "node:child_process";
import {
  closeSync,
  existsSync,
  fsyncSync,
  mkdirSync,
  openSync,
  realpathSync,
  writeSync,
} from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { setImmediate as yieldToSignals } from "node:timers/promises";

import { isolatedGit } from "./adr-files.mjs";
import { runIndustrialTests } from "./industrial-test.mjs";

const EIGHT_HOURS_MS = 8 * 60 * 60 * 1000;
const MAX_DURATION_MS = 7 * 24 * 60 * 60 * 1000;
const workspace = fileURLToPath(new URL("../", import.meta.url));

class SetupError extends Error {}

export function validateFixtures(env, disposable) {
  if (!disposable)
    throw new SetupError(
      "--disposable-databases is required; this run repeatedly writes and restores test data",
    );
  const variables = [
    "ACKPLANE_TEST_DATABASE_URL",
    "ACKPLANE_TEST_REHEARSAL_DATABASE_URL",
  ];
  const databases = variables.map((name) => {
    try {
      const url = new URL(env[name]);
      if (
        !["postgres:", "postgresql:"].includes(url.protocol) ||
        !["127.0.0.1", "localhost", "[::1]"].includes(url.hostname) ||
        url.search ||
        url.hash
      )
        throw new Error();
      return {
        host: url.host,
        name: decodeURIComponent(url.pathname.slice(1)),
      };
    } catch {
      throw new SetupError(
        `${name} must explicitly name a loopback PostgreSQL fixture without query options`,
      );
    }
  });
  if (databases[0].name !== "ackplane_test")
    throw new SetupError("the primary fixture must be named ackplane_test");
  if (
    !/^[a-zA-Z0-9_]+_rehearsal$/.test(databases[1].name) ||
    databases[0].host !== databases[1].host
  ) {
    throw new SetupError(
      "use a separate *_rehearsal database on the same disposable PostgreSQL instance",
    );
  }
  return databases;
}

export function readSource(directory = workspace) {
  const git = (args) => {
    const output = isolatedGit(args, directory);
    if (output === null) {
      throw new Error("cannot read endurance source checkout with Git");
    }
    return output;
  };
  const [commit, tree] = git(["rev-parse", "HEAD", "HEAD^{tree}"]).split(
    /\r?\n/,
  );
  return {
    commit,
    tree,
    clean: git(["status", "--porcelain", "--untracked-files=all"]) === "",
  };
}

function validSource(source) {
  return (
    source?.clean === true &&
    /^[a-f0-9]{40}$|^[a-f0-9]{64}$/.test(source.commit ?? "") &&
    /^[a-f0-9]{40}$|^[a-f0-9]{64}$/.test(source.tree ?? "")
  );
}

export async function runSoak({
  durationMs = EIGHT_HOURS_MS,
  now = () => performance.now(),
  timestamp = () => new Date().toISOString(),
  readSource,
  runCycle,
  record,
  interrupted = () => false,
}) {
  if (
    !Number.isFinite(durationMs) ||
    durationMs <= 0 ||
    durationMs > MAX_DURATION_MS
  ) {
    throw new Error("duration must be positive and no longer than seven days");
  }
  const started = now();
  let completedCycles = 0;
  let measuredMs = 0;
  let source;
  const finish = (status, exitCode, reason) => {
    const result = {
      kind: "finished",
      status,
      exitCode,
      reason,
      completedCycles,
      measuredMs,
      elapsedMs: Math.max(0, now() - started),
      recordedAt: timestamp(),
    };
    record(result);
    return result;
  };
  try {
    const current = readSource();
    source = {
      commit: current.commit,
      tree: current.tree,
      clean: current.clean,
    };
  } catch {
    return finish("failed", 1, "source_unreadable");
  }
  if (!validSource(source)) {
    return finish("failed", 1, "source_not_clean");
  }
  record({
    kind: "started",
    schema: 1,
    source,
    durationMs,
    recordedAt: timestamp(),
  });
  const sameSource = () => {
    try {
      const current = readSource();
      return (
        validSource(current) &&
        current.commit === source.commit &&
        current.tree === source.tree
      );
    } catch {
      return false;
    }
  };
  while (measuredMs < durationMs || completedCycles < 2) {
    if (interrupted()) return finish("interrupted", 130, "operator_interrupt");
    if (!sameSource()) return finish("failed", 1, "source_changed");
    const cycle = completedCycles + 1;
    const cycleStarted = now();
    if (!Number.isFinite(cycleStarted) || cycleStarted < started) {
      return finish("failed", 1, "invalid_clock");
    }
    record({ kind: "cycle_started", cycle, recordedAt: timestamp() });
    let exitCode;
    let reason = "cycle_failed";
    try {
      exitCode = await runCycle(cycle);
    } catch {
      exitCode = 1;
      reason = "cycle_exception";
    }
    const cycleMs = now() - cycleStarted;
    record({
      kind: "cycle_finished",
      cycle,
      exitCode,
      cycleMs,
      recordedAt: timestamp(),
    });
    if (interrupted()) return finish("interrupted", 130, "operator_interrupt");
    if (exitCode !== 0) {
      return finish(
        "failed",
        Number.isInteger(exitCode) && exitCode > 0 && exitCode <= 255
          ? exitCode
          : 1,
        reason,
      );
    }
    if (!Number.isFinite(cycleMs) || cycleMs <= 0)
      return finish("failed", 1, "invalid_clock");
    if (!sameSource()) return finish("failed", 1, "source_changed");
    completedCycles += 1;
    measuredMs += cycleMs;
  }
  return finish("passed", 0, "duration_completed");
}

async function main() {
  const { values } = parseArgs({
    options: {
      hours: { type: "string", default: "8" },
      "disposable-databases": { type: "boolean", default: false },
      help: { type: "boolean", short: "h" },
    },
  });
  if (values.help) {
    console.log(
      "Usage: node scripts/industrial-soak.mjs --hours 8 --disposable-databases\nRequires an isolated loopback ackplane_test database and a separate *_rehearsal database in ACKPLANE_TEST_DATABASE_URL and ACKPLANE_TEST_REHEARSAL_DATABASE_URL. Repeats the complete Industrial gate from a clean pinned checkout. Reports and isolated build output go under target/industrial-soak. This measures regression endurance, not service uptime or the seven-day pilot.",
    );
    return;
  }
  const durationMs = Number(values.hours) * 60 * 60 * 1000;
  if (
    !Number.isFinite(durationMs) ||
    durationMs <= 0 ||
    durationMs > MAX_DURATION_MS
  ) {
    throw new SetupError("--hours must be positive and no greater than 168");
  }
  validateFixtures(process.env, values["disposable-databases"]);
  if (!validSource(readSource()))
    throw new SetupError("a clean committed checkout is required");
  const runId = `${Date.now()}-${process.pid}`;
  const directory = join(workspace, "target", "industrial-soak", runId);
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  const journal = openSync(join(directory, "events.jsonl"), "wx", 0o600);
  let interrupted = false;
  const interrupt = () => {
    interrupted = true;
  };
  process.on("SIGINT", interrupt);
  process.on("SIGTERM", interrupt);
  const record = (event) => {
    writeSync(journal, `${JSON.stringify(event)}\n`);
    fsyncSync(journal);
    if (
      ["started", "cycle_started", "cycle_finished", "finished"].includes(
        event.kind,
      )
    ) {
      console.log(`industrial-soak: ${JSON.stringify(event)}`);
    }
  };
  console.log(`industrial-soak: journal ${join(directory, "events.jsonl")}`);
  const env = {
    ...process.env,
    CARGO_TARGET_DIR: join(directory, "build"),
    RUST_TEST_THREADS: "4",
    CARGO_BUILD_JOBS: "2",
  };
  try {
    const result = await runSoak({
      durationMs,
      readSource,
      record,
      interrupted: () => interrupted,
      runCycle: async (cycle) => {
        await yieldToSignals();
        if (interrupted) return 130;
        const exitCode = runIndustrialTests({
          env,
          report: (message) => console.log(message),
          error: (message) => console.error(message),
          run: (command, args, options) => {
            if (interrupted) return { status: 130 };
            record({
              kind: "command_started",
              cycle,
              command,
              args,
              recordedAt: new Date().toISOString(),
            });
            const started = performance.now();
            const outcome = spawnSync(command, args, options);
            record({
              kind: "command_finished",
              cycle,
              command,
              args,
              exitCode: outcome.status,
              signal: outcome.signal,
              errorCode: outcome.error?.code,
              elapsedMs: performance.now() - started,
              recordedAt: new Date().toISOString(),
            });
            return outcome;
          },
        });
        await yieldToSignals();
        return exitCode;
      },
    });
    process.exitCode = result.exitCode;
  } finally {
    process.off("SIGINT", interrupt);
    process.off("SIGTERM", interrupt);
    closeSync(journal);
  }
}

if (
  process.argv[1] &&
  existsSync(process.argv[1]) &&
  realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))
) {
  main().catch((error) => {
    console.error(
      error instanceof SetupError
        ? `industrial-soak: ${error.message}`
        : "industrial-soak: failed before a complete verdict; inspect setup and any unfinished journal. No passing run is recorded.",
    );
    process.exitCode = 1;
  });
}
