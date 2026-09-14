import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { readSource, runSoak, validateFixtures } from "./industrial-soak.mjs";

const source = {
  commit: "a".repeat(40),
  tree: "b".repeat(40),
  clean: true,
};

test("a passing soak measures completed work for the requested duration", async () => {
  let elapsed = 0;
  const events = [];
  const result = await runSoak({
    durationMs: 100,
    now: () => elapsed,
    readSource: () => source,
    record: (event) => events.push(structuredClone(event)),
    runCycle: () => {
      elapsed += 40;
      return 0;
    },
  });
  assert.equal(result.status, "passed");
  assert.equal(result.completedCycles, 3);
  assert.equal(result.measuredMs, 120);
  assert.equal(result.elapsedMs, 120);
  assert.equal(events[0].kind, "started");
  assert.equal(events.at(-1).kind, "finished");
  assert.equal(events[0].source.commit, source.commit);
  assert.deepEqual(
    events
      .filter((event) => event.kind === "cycle_finished")
      .map((event) => event.exitCode),
    [0, 0, 0],
  );
});

test("one failed cycle stops the run and never earns a passing duration", async () => {
  let elapsed = 0;
  let attempts = 0;
  const events = [];
  const result = await runSoak({
    durationMs: 100,
    now: () => elapsed,
    readSource: () => source,
    record: (event) => events.push(structuredClone(event)),
    runCycle: () => {
      elapsed += 60;
      attempts += 1;
      return attempts === 1 ? 0 : 17;
    },
  });
  assert.equal(result.status, "failed");
  assert.equal(result.completedCycles, 1);
  assert.equal(result.measuredMs, 60);
  assert.equal(result.exitCode, 17);
  assert.equal(attempts, 2);
  assert.equal(events.at(-1).status, "failed");
});

test("source mutation during a successful cycle invalidates the run", async () => {
  let elapsed = 0;
  let current = source;
  const result = await runSoak({
    durationMs: 1,
    now: () => elapsed,
    readSource: () => current,
    record: () => {},
    runCycle: () => {
      elapsed += 100;
      current = { ...source, tree: "c".repeat(40) };
      return 0;
    },
  });
  assert.equal(result.status, "failed");
  assert.equal(result.reason, "source_changed");
  assert.equal(result.completedCycles, 0);
});

test("dirty and unreadable sources never run a command", async () => {
  for (const readSource of [
    () => ({ ...source, clean: false }),
    () => {
      throw new Error("private-path");
    },
  ]) {
    const events = [];
    const result = await runSoak({
      durationMs: 1,
      readSource,
      record: (event) => events.push(event),
      runCycle: () => assert.fail("source must be verified first"),
    });
    assert.equal(result.status, "failed");
    assert.doesNotMatch(JSON.stringify(events), /private-path/);
  }
});

test("an interrupted run is not counted even if the cycle returned zero", async () => {
  let elapsed = 0;
  let cancelled = false;
  const result = await runSoak({
    durationMs: 1,
    now: () => elapsed,
    readSource: () => source,
    record: () => {},
    interrupted: () => cancelled,
    runCycle: () => {
      elapsed += 100;
      cancelled = true;
      return 0;
    },
  });
  assert.equal(result.status, "interrupted");
  assert.equal(result.completedCycles, 0);
  assert.equal(result.exitCode, 130);
});

test("no progress, exceptions and invalid exit values cannot pass", async () => {
  for (const runCycle of [
    () => 0,
    () => undefined,
    () => {
      throw new Error("secret-value");
    },
  ]) {
    const events = [];
    const result = await runSoak({
      durationMs: 1,
      now: () => 0,
      readSource: () => source,
      record: (event) => events.push(event),
      runCycle,
    });
    assert.equal(result.status, "failed");
    assert.doesNotMatch(JSON.stringify(events), /secret-value/);
  }
});

test("journal failure prevents work instead of losing the run's provenance", async () => {
  await assert.rejects(
    runSoak({
      durationMs: 1,
      readSource: () => source,
      record: () => {
        throw new Error("journal unavailable");
      },
      runCycle: () => assert.fail("record start first"),
    }),
    /journal unavailable/,
  );
});

test("database acknowledgement and isolated names are required without exposing credentials", () => {
  const env = {
    ACKPLANE_TEST_DATABASE_URL:
      "postgresql://test:secret@127.0.0.1:15432/ackplane_test",
    ACKPLANE_TEST_REHEARSAL_DATABASE_URL:
      "postgresql://test:secret@127.0.0.1:15432/ackplane_rehearsal",
  };
  assert.equal(validateFixtures(env, true).length, 2);
  assert.throws(() => validateFixtures(env, false), /disposable/);
  for (const primary of [
    undefined,
    "postgresql://test:secret@production/ackplane_test",
    "postgresql://test:secret@127.0.0.1:15432/live",
    "postgresql://test:secret@127.0.0.1:15432/ackplane_test?host=production",
  ]) {
    assert.throws(
      () =>
        validateFixtures({ ...env, ACKPLANE_TEST_DATABASE_URL: primary }, true),
      (error) => {
        assert.doesNotMatch(error.message, /secret/);
        return true;
      },
    );
  }
  assert.throws(
    () =>
      validateFixtures(
        {
          ...env,
          ACKPLANE_TEST_REHEARSAL_DATABASE_URL: env.ACKPLANE_TEST_DATABASE_URL,
        },
        true,
      ),
    /separate/,
  );
});

test("duration requires positive finite input and at least two successful cycles", async () => {
  for (const durationMs of [0, -1, Infinity, NaN, 8 * 24 * 60 * 60 * 1000]) {
    await assert.rejects(
      runSoak({
        durationMs,
        runCycle: () => assert.fail("invalid duration cannot start"),
      }),
      /duration/,
    );
  }
  let elapsed = 0;
  const result = await runSoak({
    durationMs: 1,
    now: () => elapsed,
    readSource: () => source,
    record: () => {},
    runCycle: () => {
      elapsed += 100;
      return 0;
    },
  });
  assert.equal(result.completedCycles, 2);
  assert.equal(result.measuredMs, 200);
});

test("readSource detects real tracked and untracked Git changes", (context) => {
  const directory = mkdtempSync(join(tmpdir(), "industrial-soak-source-"));
  context.after(() => rmSync(directory, { recursive: true, force: true }));
  const git = (args) =>
    execFileSync("git", args, { cwd: directory, stdio: "pipe" });
  git(["init", "--quiet"]);
  writeFileSync(join(directory, "candidate.txt"), "initial\n");
  git(["add", "candidate.txt"]);
  git([
    "-c",
    "user.name=Soak Test",
    "-c",
    "user.email=soak@example.invalid",
    "-c",
    "commit.gpgsign=false",
    "commit",
    "--quiet",
    "-m",
    "fixture",
  ]);
  const initial = readSource(directory);
  assert.equal(initial.clean, true);
  assert.match(initial.commit, /^[a-f0-9]{40}$|^[a-f0-9]{64}$/);
  writeFileSync(join(directory, "candidate.txt"), "changed\n");
  assert.equal(readSource(directory).clean, false);
  writeFileSync(join(directory, "candidate.txt"), "initial\n");
  writeFileSync(join(directory, "untracked.txt"), "another input\n");
  assert.equal(readSource(directory).clean, false);
  rmSync(join(directory, "untracked.txt"));
  assert.deepEqual(readSource(directory), initial);
});

test("the executable explains missing fixture consent without printing database credentials", () => {
  const script = fileURLToPath(
    new URL("./industrial-soak.mjs", import.meta.url),
  );
  const result = spawnSync(process.execPath, [script], {
    encoding: "utf8",
    env: {
      ...process.env,
      ACKPLANE_TEST_DATABASE_URL:
        "postgresql://private:private-password@production/live",
    },
  });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /--disposable-databases is required/);
  assert.doesNotMatch(result.stderr, /private-password/);
  const help = spawnSync(process.execPath, [script, "--help"], {
    encoding: "utf8",
  });
  assert.equal(help.status, 0);
  assert.match(help.stdout, /not service uptime/);
});
