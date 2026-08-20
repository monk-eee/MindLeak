// Reference-consumer compatibility gate (ADR-0104). CI runs a minimal
// external client -- never the servers' internal Rust APIs -- against every
// change to either MCP tool surface, so a breaking change is caught here
// instead of surfacing first as a downstream consumer's failure.
//
// Deliberately narrow per ADR-0104 decision 5: new tools are additive by
// default and never fail this gate. It only asserts that the specific tool
// families a real external consumer already depends on (one graph read, one
// task read, one knowledge read, one conformance read) still answer over the
// wire -- not that the advertised tool set is unchanged.

import { existsSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import { dirname, join } from "node:path";
import { randomBytes } from "node:crypto";
import { resolveServer } from "./claim-gate.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, "..");

/** The one representative tool this gate calls per family, per plane. */
export const REPRESENTATIVE_TOOLS = {
  mindleak: ["graph_stats"],
  lodestar: ["task_query", "active_knowledge", "conformance_history"],
};

/**
 * Which of `required` are absent from `advertised` (the live `tools/list`
 * response). Pure and order-preserving so a missing tool is reported in the
 * same order every run, not shuffled by object/array iteration.
 */
export const missingTools = (required, advertised) => {
  const names = new Set((advertised ?? []).map((tool) => tool?.name));
  return required.filter((name) => !names.has(name));
};

/**
 * Exercise one plane's representative read-only calls. Never creates or
 * mutates durable state -- every call here is a plain read, so the gate
 * proves the wire contract without polluting the ledger it just checked.
 */
const exerciseServer = async (client) => {
  await client.graph.stats();
};

const exerciseLodestar = async (client) => {
  await client.tasks.next();
  await client.knowledge.active();
  // The synthetic id is deliberate: conformance_history is a plain
  // `WHERE task_id = ?` read that returns an empty array for an id nobody
  // ever claimed, so this proves the wire path without depending on any
  // task existing in whatever database this CI run happens to start from.
  await client.evidence.history({
    task_id: "task:reference-consumer-gate-probe",
  });
};

const PLANE_EXERCISERS = {
  mindleak: exerciseServer,
  lodestar: exerciseLodestar,
};

async function checkPlane(plane, MindLeakClient) {
  const command = resolveServer(repoRoot, plane);
  if (!command || !existsSync(command)) {
    console.error(
      `[${plane}] no built binary found (set ${plane.toUpperCase()}_MCP_BIN or build target/release).`,
    );
    return false;
  }

  const client = new MindLeakClient(command);
  try {
    const identity = await client.connect({
      clientName: "reference-consumer-gate",
    });
    await client.openSession(randomBytes(16).toString("hex"));
    const tools = await client.listTools();
    if (tools.length === 0) {
      console.error(`[${plane}] tools/list returned no tools`);
      return false;
    }
    const missing = missingTools(REPRESENTATIVE_TOOLS[plane], tools);
    if (missing.length > 0) {
      console.error(
        `[${plane}] missing previously-advertised tool(s): ${missing.join(", ")}`,
      );
      return false;
    }
    await PLANE_EXERCISERS[plane](client);
    console.log(
      `[${plane}] ${identity.name} ${identity.version}: ${tools.length} tools, representative reads ok`,
    );
    return true;
  } catch (err) {
    console.error(`[${plane}] compatibility check failed: ${err.message}`);
    return false;
  } finally {
    client.close();
  }
}

async function main() {
  const clientPath = join(
    repoRoot,
    "clients",
    "node",
    "mindleak-client",
    "dist",
    "src",
    "index.js",
  );
  if (!existsSync(clientPath)) {
    console.error(
      `reference-consumer: built client not found at ${clientPath}`,
    );
    console.error(
      "reference-consumer: run npm --prefix clients/node/mindleak-client ci && npm run build first",
    );
    process.exit(1);
  }
  const { MindLeakClient } = await import(pathToFileURL(clientPath).href);

  const results = await Promise.all(
    Object.keys(REPRESENTATIVE_TOOLS).map((plane) =>
      checkPlane(plane, MindLeakClient),
    ),
  );
  process.exit(results.every(Boolean) ? 0 : 1);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
