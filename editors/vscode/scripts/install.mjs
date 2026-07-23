import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import readline from "node:readline";
import { fileURLToPath } from "node:url";

import { applyEdits, modify, parse, printParseErrorCode } from "jsonc-parser/lib/esm/main.js";

const SERVERS = [
  { name: "mindleak", binary: "mindleak-mcp", databaseVariable: "MINDLEAK_DB" },
  { name: "lodestar", binary: "lodestar-mcp", databaseVariable: "LODESTAR_DB" },
];

const DEFAULT_EMBED_URL = "http://localhost:11434/v1";
const DEFAULT_EMBED_MODEL = "nomic-embed-text";

/**
 * Best-effort check of the optional semantic-recall embedding backend (ADR-0008)
 * so a new user learns at install time whether recall works — and exactly how to
 * enable it — instead of hitting a mysterious 404 the first time they call it.
 * Never throws: recall is optional and this must not fail the install. Injectable
 * via `runtime.fetch` / `runtime.embedUrl` / `runtime.embedModel` for tests.
 */
export async function probeEmbeddingCapability(runtime = {}) {
  const url = (runtime.embedUrl ?? process.env.MINDLEAK_EMBED_URL ?? DEFAULT_EMBED_URL).replace(
    /\/+$/,
    ""
  );
  const model = runtime.embedModel ?? process.env.MINDLEAK_EMBED_MODEL ?? DEFAULT_EMBED_MODEL;
  const hint = `run \`ollama pull ${model}\` (or set MINDLEAK_EMBED_URL / MINDLEAK_EMBED_MODEL to a reachable model). Recall is optional — the rest of MindLeak works without it.`;
  const doFetch = runtime.fetch ?? globalThis.fetch;
  if (typeof doFetch !== "function") {
    return { available: false, url, model, hint };
  }
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), runtime.probeTimeoutMs ?? 3000);
  try {
    const response = await doFetch(`${url}/embeddings`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ model, input: "ok" }),
      signal: controller.signal,
    });
    return response && response.ok
      ? { available: true, url, model, hint: null }
      : { available: false, url, model, hint };
  } catch {
    return { available: false, url, model, hint };
  } finally {
    clearTimeout(timer);
  }
}

export function parseArguments(argv, cwd = process.cwd()) {
  const options = { workspace: cwd, agent: "copilot", version: undefined, force: false };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--force") {
      options.force = true;
    } else if (["--workspace", "--agent", "--version"].includes(argument)) {
      const value = argv[index + 1];
      if (!value || value.startsWith("--")) {
        throw new Error(`${argument} requires a value`);
      }
      options[argument.slice(2)] = value;
      index += 1;
    } else if (argument === "--help" || argument === "-h") {
      options.help = true;
    } else {
      throw new Error(`unknown argument: ${argument}`);
    }
  }
  options.workspace = path.resolve(cwd, options.workspace);
  if (!options.agent.trim()) {
    throw new Error("--agent must not be empty");
  }
  if (options.version) {
    validateVersion(options.version);
  }
  return options;
}

export function registrations(version, platform, agent) {
  validateVersion(version);
  const executableExtension = platform === "win32" ? ".exe" : "";
  const installRoot = `\${workspaceFolder}/.mindleak/bin/${version}`;
  return {
    mindleak: {
      command: `${installRoot}/mindleak-mcp${executableExtension}`,
      env: {
        MINDLEAK_DB: "${workspaceFolder}/.mindleak/graph.db",
        MINDLEAK_AGENT: agent,
        MINDLEAK_WORKSPACE: "${workspaceFolder}",
      },
    },
    lodestar: {
      command: `${installRoot}/lodestar-mcp${executableExtension}`,
      env: {
        LODESTAR_DB: "${workspaceFolder}/.lodestar/spec.db",
        LODESTAR_AGENT: agent,
      },
    },
  };
}

export function updateMcpConfig(source, serverRegistrations) {
  let document = source.trim() ? source : '{\n  "servers": {}\n}\n';
  const errors = [];
  const parsed = parse(document, errors, { allowTrailingComma: true, disallowComments: false });
  if (errors.length > 0) {
    const details = errors
      .map((error) => `${printParseErrorCode(error.error)} at offset ${error.offset}`)
      .join(", ");
    throw new Error(`cannot update .vscode/mcp.json: ${details}`);
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error("cannot update .vscode/mcp.json: root must be an object");
  }

  const formattingOptions = {
    insertSpaces: true,
    tabSize: 2,
    eol: document.includes("\r\n") ? "\r\n" : "\n",
  };
  for (const [name, registration] of Object.entries(serverRegistrations)) {
    document = applyEdits(
      document,
      modify(document, ["servers", name], registration, { formattingOptions })
    );
  }
  return document.endsWith(formattingOptions.eol)
    ? document
    : `${document}${formattingOptions.eol}`;
}

export function updateGitignore(source) {
  const eol = source.includes("\r\n") ? "\r\n" : "\n";
  const lines = source.split(/\r?\n/);
  const missing = [".mindleak/", ".lodestar/*", "!.lodestar/CONSTITUTION.md"].filter(
    (rule) => !lines.includes(rule)
  );
  if (missing.length === 0) {
    return source;
  }
  const prefix = source.length > 0 && !source.endsWith("\n") ? eol : "";
  const separator = source.trim() ? eol : "";
  return `${source}${prefix}${separator}# MindLeak local state${eol}${missing.join(eol)}${eol}`;
}

export async function install(
  options,
  archiveDirectory = path.dirname(fileURLToPath(import.meta.url)),
  runtime = {}
) {
  const version = options.version ?? readVersion(archiveDirectory);
  validateVersion(version);
  const platform = runtime.platform ?? process.platform;
  const executableExtension = runtime.executableExtension ?? (platform === "win32" ? ".exe" : "");
  const smoke = runtime.smoke ?? smokeInstalledServers;
  const installParent = path.join(options.workspace, ".mindleak", "bin");
  const installDirectory = path.join(installParent, version);
  fs.mkdirSync(installParent, { recursive: true });

  if (fs.existsSync(installDirectory) && !options.force) {
    await smoke(installDirectory, executableExtension);
    console.log(`Using existing MindLeak ${version} installation`);
  } else {
    await stageInstallation(
      archiveDirectory,
      installDirectory,
      executableExtension,
      options.force,
      smoke
    );
  }

  const vscodeDirectory = path.join(options.workspace, ".vscode");
  const configPath = path.join(vscodeDirectory, "mcp.json");
  const currentConfig = fs.existsSync(configPath) ? fs.readFileSync(configPath, "utf8") : "";
  const nextConfig = updateMcpConfig(
    currentConfig,
    registrations(version, platform, options.agent)
  );
  fs.mkdirSync(vscodeDirectory, { recursive: true });
  atomicWrite(configPath, nextConfig);

  const gitignorePath = path.join(options.workspace, ".gitignore");
  const currentGitignore = fs.existsSync(gitignorePath)
    ? fs.readFileSync(gitignorePath, "utf8")
    : "";
  const nextGitignore = updateGitignore(currentGitignore);
  if (nextGitignore !== currentGitignore) {
    atomicWrite(gitignorePath, nextGitignore);
  }

  console.log(`Installed MindLeak ${version} in ${installDirectory}`);
  console.log(`Registered mindleak and lodestar in ${configPath}`);

  const probeEmbedding = runtime.probeEmbedding ?? probeEmbeddingCapability;
  const recall = await probeEmbedding(runtime);
  if (recall.available) {
    console.log(`Semantic recall: enabled (${recall.model} at ${recall.url}).`);
  } else {
    console.log(`Semantic recall: disabled (optional) — to enable it, ${recall.hint}`);
  }
}

async function stageInstallation(
  archiveDirectory,
  installDirectory,
  executableExtension,
  force,
  smoke
) {
  const staging = `${installDirectory}.partial-${process.pid}`;
  fs.rmSync(staging, { recursive: true, force: true });
  fs.mkdirSync(staging, { recursive: true });
  try {
    for (const server of SERVERS) {
      const fileName = `${server.binary}${executableExtension}`;
      const source = path.join(archiveDirectory, fileName);
      if (!fs.existsSync(source)) {
        throw new Error(`release archive is missing ${fileName}`);
      }
      const destination = path.join(staging, fileName);
      fs.copyFileSync(source, destination);
      if (process.platform !== "win32") {
        fs.chmodSync(destination, 0o755);
      }
    }
    await smoke(staging, executableExtension);

    if (fs.existsSync(installDirectory)) {
      if (!force) {
        throw new Error(`installation already exists: ${installDirectory}`);
      }
      const previous = `${installDirectory}.previous-${process.pid}`;
      fs.rmSync(previous, { recursive: true, force: true });
      fs.renameSync(installDirectory, previous);
      try {
        fs.renameSync(staging, installDirectory);
        fs.rmSync(previous, { recursive: true, force: true });
      } catch (error) {
        if (!fs.existsSync(installDirectory) && fs.existsSync(previous)) {
          fs.renameSync(previous, installDirectory);
        }
        throw error;
      }
    } else {
      fs.renameSync(staging, installDirectory);
    }
  } finally {
    fs.rmSync(staging, { recursive: true, force: true });
  }
}

async function smokeInstalledServers(directory, executableExtension) {
  for (const server of SERVERS) {
    await smokeServer(
      path.join(directory, `${server.binary}${executableExtension}`),
      server.databaseVariable
    );
  }
}

export function smokeServer(
  binary,
  databaseVariable,
  spawnProcess = spawn,
  timeoutMilliseconds = 10_000
) {
  return new Promise((resolve, reject) => {
    const child = spawnProcess(binary, [], {
      env: { ...process.env, [databaseVariable]: ":memory:", MINDLEAK_LOG: "off" },
      stdio: ["pipe", "pipe", "pipe"],
    });
    let stderr = "";
    let settled = false;
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
      const complete = () => (error ? reject(error) : resolve());
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

function readVersion(archiveDirectory) {
  const versionPath = path.join(archiveDirectory, "VERSION");
  if (!fs.existsSync(versionPath)) {
    throw new Error("release archive is missing VERSION; pass --version for a local build");
  }
  return fs.readFileSync(versionPath, "utf8").trim();
}

function validateVersion(version) {
  if (!/^v?\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
    throw new Error(`invalid release version: ${version}`);
  }
}

function atomicWrite(destination, content) {
  const temporary = `${destination}.partial-${process.pid}`;
  try {
    fs.writeFileSync(temporary, content, "utf8");
    fs.renameSync(temporary, destination);
  } finally {
    if (fs.existsSync(temporary)) {
      fs.rmSync(temporary, { force: true });
    }
  }
}

function help() {
  return [
    "Usage: node install.mjs [options]",
    "",
    "Options:",
    "  --workspace <path>  Workspace to install and register (default: cwd)",
    "  --agent <id>        Stable MCP agent id (default: copilot)",
    "  --version <tag>     Release version override for local bundles",
    "  --force             Replace an invalid/existing version directory",
    "  --help              Show this help",
  ].join("\n");
}

const invokedDirectly =
  process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url));
if (invokedDirectly) {
  try {
    const options = parseArguments(process.argv.slice(2));
    if (options.help) {
      console.log(help());
    } else {
      await install(options);
    }
  } catch (error) {
    console.error(`MindLeak installation failed: ${error.message}`);
    process.exitCode = 1;
  }
}
