import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const quickstart = readFileSync(
  new URL("../docs/INDUSTRIAL-QUICKSTART.md", import.meta.url),
  "utf8",
);
const blocks = [...quickstart.matchAll(/```([^\n]*)\n([\s\S]*?)```/g)];
const commands = blocks
  .flatMap(([, , body]) => body.replace(/\\\r?\n\s*/g, " ").split(/\r?\n/))
  .filter((line) =>
    /register-me(?:\.exe)?\s+(request|approve|activate|serve)\b/.test(line),
  );

test("Industrial first-run commands use portable preparation and launchers", () => {
  assert.match(
    quickstart,
    /node scripts\/ackplane-compose\.mjs prepare ABSOLUTE_CONFIG_DIR/,
  );
  assert.match(
    quickstart,
    /node scripts\/run-ackplane\.mjs supervisor --workers ABSOLUTE_WORKERS_FILE/,
  );
  for (const command of commands) {
    assert.match(command, /^node scripts\/run-ackplane\.mjs register-me /);
  }
  const examples = blocks.map(([, , body]) => body).join("\n");
  assert.doesNotMatch(
    examples,
    /(^|\n)\s*(export |\$env:|BIN=|mkdir -p)|\$BIN/,
  );
  assert.doesNotMatch(
    examples,
    /MINDLEAK_ACKPLANE_(NODE_ID|SIGNING_KEY_ID|NODE_SIGNING_KEY_SEED|KEY_PATH)\s*[=:]/,
  );
});

// The old quickstart omitted provider state and configured obsolete consumer
// signing fields, so following it could never start the companion-owned runtime.
test("Industrial enrollment examples use the current provider and companion CLI contract", () => {
  const cli = readFileSync(
    new URL(
      "../crates/ackplane-server/src/bin/register-me/main.rs",
      import.meta.url,
    ),
    "utf8",
  );
  const knownFlags = new Set(
    [...cli.matchAll(/--[a-z][a-z-]*/g)].map(([flag]) => flag),
  );
  for (const operation of ["request", "approve", "activate", "serve"]) {
    const examples = commands.filter((line) =>
      new RegExp(`register-me(?:\\.exe)?\\s+${operation}\\b`).test(line),
    );
    assert.ok(
      examples.length,
      `the quickstart must include register-me ${operation}`,
    );
    for (const command of examples) {
      for (const [flag] of command.matchAll(/--[a-z][a-z-]*/g)) {
        assert.ok(
          knownFlags.has(flag),
          `${operation} documents an unknown CLI flag ${flag}`,
        );
      }
      if (operation !== "approve") {
        assert.match(
          command,
          /--state-dir\s+\S+/,
          `${operation} must name its provider state`,
        );
      }
      if (operation === "request") {
        assert.match(command, /--provider\s+credential-facility-software\b/);
      }
      if (operation === "approve") {
        assert.match(command, /--fingerprint\s+\S+/);
        assert.match(command, /--admin-database-url\s+\S+/);
      } else {
        assert.doesNotMatch(command, /--admin-database-url/);
      }
      assert.doesNotMatch(
        command,
        /--skip-sync|--key-path|--node-signing-key-seed/,
      );
    }
  }
});

test("Industrial MCP examples use companion directory and scope instead of private identity settings", () => {
  const configurations = blocks
    .filter(
      ([, language, body]) =>
        ["json", "jsonc"].includes(language.trim()) &&
        body.includes('"servers"'),
    )
    .map(([, , body]) => JSON.parse(body));
  assert.ok(
    configurations.length,
    "the quickstart must include a ready-to-edit MCP configuration",
  );
  for (const configuration of configurations) {
    for (const server of Object.values(configuration.servers)) {
      assert.ok(server.cwd, "MCP consumers must declare their workspace");
      for (const name of [
        "MINDLEAK_ACKPLANE_STATE_DIR",
        "MINDLEAK_ACKPLANE_TENANT_ID",
        "MINDLEAK_ACKPLANE_REPOSITORY_ID",
      ]) {
        assert.ok(server.env[name], `MCP consumer must configure ${name}`);
      }
      for (const name of [
        "MINDLEAK_ACKPLANE_NODE_ID",
        "MINDLEAK_ACKPLANE_SIGNING_KEY_ID",
        "MINDLEAK_ACKPLANE_NODE_SIGNING_KEY_SEED",
        "MINDLEAK_ACKPLANE_KEY_PATH",
        "MINDLEAK_ACKPLANE_TLS_CA_PATH",
      ]) {
        assert.equal(
          server.env[name],
          undefined,
          `${name} belongs to the companion, not consumers`,
        );
      }
    }
  }
});
