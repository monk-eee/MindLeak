import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";

import { smokeServer, SERVERS } from "./mcp-smoke.mjs";

const extensionRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** `serverInfo.version` is `<crate version>+<build sha>` (e.g. "0.1.7-alpha+abc123..."). */
export function shaFromVersion(version) {
  const separator = version.lastIndexOf("+");
  return separator >= 0 ? version.slice(separator + 1) : undefined;
}

/**
 * A staged binary can be older than the checkout it is packaged alongside
 * when someone forgets to rebuild before staging, and the packaged copy
 * carries no other version or provenance marker to catch that. Pure over the
 * two identities being compared, so the decision is testable without
 * spawning a real binary or a real git process: `undefined` when either
 * identity is missing (nothing to compare) or the binary's own reported sha
 * is a prefix of `headSha` (it was built at the commit being packaged); the
 * warning string otherwise.
 */
export function stagedIdentityMismatchWarning(fileName, version, headSha) {
  if (!version || !headSha) {
    return undefined;
  }
  const builtSha = shaFromVersion(version);
  if (!builtSha || headSha.startsWith(builtSha)) {
    return undefined;
  }
  return (
    `WARNING: staged ${fileName} reports version ${version} (built from ${builtSha}), ` +
    `but this checkout's HEAD is ${headSha.slice(0, 12)} -- this binary was not rebuilt at the ` +
    `commit being packaged and will ship stale if this is part of a release.`
  );
}

function currentHeadSha(cwd) {
  try {
    return execFileSync("git", ["rev-parse", "HEAD"], { cwd, encoding: "utf8" }).trim();
  } catch (error) {
    console.warn(`Could not determine the current git HEAD: ${error.message}`);
    return undefined;
  }
}

async function main() {
  const source = path.resolve(
    readArgument("--source") ?? path.join(extensionRoot, "..", "..", "target", "release")
  );
  const executableExtension =
    readArgument("--extension") ?? (process.platform === "win32" ? ".exe" : "");
  const destination = path.join(extensionRoot, "bin");

  fs.rmSync(destination, { recursive: true, force: true });
  fs.mkdirSync(destination, { recursive: true });

  const headSha = currentHeadSha(extensionRoot);
  const staged = {};

  for (const server of SERVERS) {
    const fileName = `${server.binary}${executableExtension}`;
    const sourcePath = path.join(source, fileName);
    if (!fs.existsSync(sourcePath)) {
      throw new Error(`native server not found: ${sourcePath}`);
    }
    const destinationPath = path.join(destination, fileName);
    fs.copyFileSync(sourcePath, destinationPath);
    if (process.platform !== "win32") {
      fs.chmodSync(destinationPath, 0o755);
    }
    console.log(`Staged ${fileName}`);

    const identity = await smokeServer(destinationPath, server.databaseVariable).catch((error) => {
      console.warn(`Could not read ${fileName}'s reported version: ${error.message}`);
      return undefined;
    });
    staged[server.binary] = { version: identity?.version ?? null };

    const warning = stagedIdentityMismatchWarning(fileName, identity?.version, headSha);
    if (warning) {
      console.warn(warning);
    }
  }

  fs.writeFileSync(
    path.join(destination, "build-info.json"),
    `${JSON.stringify({ stagedFromCommit: headSha, servers: staged }, null, 2)}\n`
  );
}

function readArgument(name) {
  const index = process.argv.indexOf(name);
  if (index < 0) {
    return undefined;
  }
  const value = process.argv[index + 1];
  if (!value || value.startsWith("--")) {
    throw new Error(`${name} requires a value`);
  }
  return value;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  main().catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
