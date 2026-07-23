import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const extensionRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const source = path.resolve(
  readArgument("--source") ?? path.join(extensionRoot, "..", "..", "target", "release")
);
const executableExtension =
  readArgument("--extension") ?? (process.platform === "win32" ? ".exe" : "");
const destination = path.join(extensionRoot, "bin");

fs.rmSync(destination, { recursive: true, force: true });
fs.mkdirSync(destination, { recursive: true });

for (const server of ["mindleak-mcp", "lodestar-mcp"]) {
  const fileName = `${server}${executableExtension}`;
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
