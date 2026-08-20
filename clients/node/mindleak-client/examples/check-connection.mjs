#!/usr/bin/env node
// Live smoke example: connects to both real MindLeak servers using the typed
// client (not the raw wire protocol) and lists their tools. Requires MindLeak
// installed locally -- see MINDLEAK_MCP_BIN / LODESTAR_MCP_BIN below.
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { randomBytes } from "node:crypto";
import { MindLeakClient } from "../dist/src/index.js";

const DEFAULT_BIN_DIR = join(homedir(), ".mindleak", "bin");

function resolveBinary(envVar, name) {
  const override = process.env[envVar];
  if (override) return override;
  const candidate = process.platform === "win32" ? `${name}.exe` : name;
  return join(DEFAULT_BIN_DIR, candidate);
}

async function checkServer(label, command) {
  if (!existsSync(command)) {
    console.error(`[${label}] not found at ${command}; set the matching *_MCP_BIN env var.`);
    return false;
  }
  const client = new MindLeakClient(command);
  try {
    const serverInfo = await client.connect({ clientName: "mindleak-client-example" });
    await client.openSession(randomBytes(16).toString("hex"));
    const tools = await client.listTools();
    console.log(`[${label}] connected: ${serverInfo.name} ${serverInfo.version} (${tools.length} tools)`);
    return true;
  } catch (err) {
    console.error(`[${label}] connection failed: ${err.message}`);
    return false;
  } finally {
    client.close();
  }
}

const mindleakBin = resolveBinary("MINDLEAK_MCP_BIN", "mindleak-mcp");
const lodestarBin = resolveBinary("LODESTAR_MCP_BIN", "lodestar-mcp");

const results = await Promise.all([
  checkServer("mindleak-mcp", mindleakBin),
  checkServer("lodestar-mcp", lodestarBin),
]);

process.exit(results.every(Boolean) ? 0 : 1);
