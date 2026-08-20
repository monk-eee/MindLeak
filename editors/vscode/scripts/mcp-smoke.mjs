import { spawn } from "node:child_process";
import path from "node:path";
import readline from "node:readline";

// Split out of install.mjs so a caller that only needs to smoke-test a server
// (e.g. stage-native.mjs) does not pull in install.mjs's jsonc-parser
// dependency for a JSON-RPC handshake that has nothing to do with it.
export const SERVERS = [
  { name: "mindleak", binary: "mindleak-mcp", databaseVariable: "MINDLEAK_DB" },
  { name: "lodestar", binary: "lodestar-mcp", databaseVariable: "LODESTAR_DB" },
];

/**
 * Spawn `binary`, complete an MCP `initialize`/`tools/list` handshake, then
 * stop it. Resolves with the server's self-reported `{ name, version }` (the
 * same identity a live MCP client sees), or `undefined` if the handshake
 * response carried no `serverInfo`. Rejects if the handshake fails or times
 * out.
 */
export function smokeServer(
  binary,
  databaseVariable,
  spawnProcess = spawn,
  timeoutMilliseconds = 10_000
) {
  return new Promise((resolve, reject) => {
    const child = spawnProcess(binary, [], {
      env: {
        ...process.env,
        [databaseVariable]: ":memory:",
        MINDLEAK_LOG: "off",
        MINDLEAK_AUTONOMOUS_PRUNE: "false",
        MINDLEAK_AUTONOMOUS_CONSOLIDATION: "false",
        MINDLEAK_AUTONOMOUS_INDEX: "false",
      },
      stdio: ["pipe", "pipe", "pipe"],
    });
    let stderr = "";
    let settled = false;
    let identity;
    const lines = readline.createInterface({ input: child.stdout });
    const timer = setTimeout(
      () => finish(new Error(`${path.basename(binary)} smoke test timed out: ${stderr}`)),
      timeoutMilliseconds
    );
    const finish = (error) => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(timer);
      lines.close();
      const complete = () => (error ? reject(error) : resolve(identity));
      if (child.exitCode !== null && child.exitCode !== undefined) {
        complete();
        return;
      }
      child.once("exit", complete);
      if (!child.kill()) {
        child.removeListener("exit", complete);
        complete();
      }
    };

    child.on("error", finish);
    child.on("exit", (code) => {
      if (!settled) {
        finish(new Error(`${path.basename(binary)} exited before MCP ready (code ${code})`));
      }
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
    });
    lines.on("line", (line) => {
      let message;
      try {
        message = JSON.parse(line);
      } catch {
        finish(new Error(`${path.basename(binary)} emitted invalid JSON: ${line}`));
        return;
      }
      if (message.id === 1 && message.result) {
        const info = message.result.serverInfo;
        identity =
          typeof info?.name === "string" && typeof info?.version === "string"
            ? { name: info.name, version: info.version }
            : undefined;
        child.stdin.write(
          `${JSON.stringify({ jsonrpc: "2.0", id: 2, method: "tools/list", params: {} })}\n`
        );
      } else if (message.id === 2) {
        if (!Array.isArray(message.result?.tools) || message.result.tools.length === 0) {
          finish(new Error(`${path.basename(binary)} returned no MCP tools`));
        } else {
          finish();
        }
      } else if (message.error) {
        finish(new Error(`${path.basename(binary)} returned ${JSON.stringify(message.error)}`));
      }
    });
    child.stdin.write(
      `${JSON.stringify({
        jsonrpc: "2.0",
        id: 1,
        method: "initialize",
        params: {
          protocolVersion: "2024-11-05",
          capabilities: {},
          clientInfo: { name: "mindleak-installer", version: "1" },
        },
      })}\n`
    );
  });
}
