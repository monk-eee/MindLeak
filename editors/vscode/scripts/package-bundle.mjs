import fs from "node:fs";
import path from "node:path";
import process from "node:process";

import yazl from "yazl";

const sourceDirectory = path.resolve(requiredArgument("--source"));
const destination = path.resolve(requiredArgument("--out"));
const files = valuesAfter("--files");
if (files.length === 0) {
  throw new Error("--files requires at least one file name");
}

fs.mkdirSync(path.dirname(destination), { recursive: true });
const archive = new yazl.ZipFile();
for (const fileName of files) {
  if (path.basename(fileName) !== fileName) {
    throw new Error(`archive entry must be a file name: ${fileName}`);
  }
  const source = path.join(sourceDirectory, fileName);
  if (!fs.statSync(source).isFile()) {
    throw new Error(`release bundle input is not a file: ${source}`);
  }
  const executable =
    fileName === "install.mjs" || fileName === "mindleak-mcp" || fileName === "lodestar-mcp";
  archive.addFile(source, fileName, {
    mtime: new Date(0),
    mode: executable ? 0o100755 : 0o100644,
  });
}

await new Promise((resolve, reject) => {
  const output = fs.createWriteStream(destination, { flags: "wx" });
  output.on("close", resolve);
  output.on("error", reject);
  archive.outputStream.on("error", reject).pipe(output);
  archive.end();
});
console.log(`Created ${destination}`);

function requiredArgument(name) {
  const index = process.argv.indexOf(name);
  const value = index >= 0 ? process.argv[index + 1] : undefined;
  if (!value || value.startsWith("--")) {
    throw new Error(`${name} requires a value`);
  }
  return value;
}

function valuesAfter(name) {
  const index = process.argv.indexOf(name);
  if (index < 0) {
    return [];
  }
  return process.argv.slice(index + 1).filter((value) => !value.startsWith("--"));
}
