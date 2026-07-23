import path from "node:path";
import process from "node:process";

import yauzl from "yauzl";

const archive = path.resolve(requiredArgument("--file"));
const executableExtension = optionalArgument("--extension") ?? "";
const entries = await listEntries(archive);
const required = [
  "extension/package.json",
  "extension/LICENSE.txt",
  "extension/out/extension.js",
  `extension/bin/mindleak-mcp${executableExtension}`,
  `extension/bin/lodestar-mcp${executableExtension}`,
];
const missing = required.filter((entry) => !entries.includes(entry));
const forbidden = entries.filter((entry) =>
  ["extension/node_modules/", "extension/coverage/", "extension/src/", "extension/scripts/"].some(
    (prefix) => entry.startsWith(prefix)
  )
);
if (missing.length > 0 || forbidden.length > 0) {
  throw new Error(
    `invalid VSIX contents; missing=${missing.join(",")}; forbidden=${forbidden.join(",")}`
  );
}
console.log(`Verified ${archive}: ${entries.length} runtime entries`);

function listEntries(file) {
  return new Promise((resolve, reject) => {
    yauzl.open(file, { lazyEntries: true }, (error, zip) => {
      if (error) {
        reject(error);
        return;
      }
      const names = [];
      zip.on("error", reject);
      zip.on("entry", (entry) => {
        names.push(entry.fileName);
        zip.readEntry();
      });
      zip.on("end", () => resolve(names));
      zip.readEntry();
    });
  });
}

function requiredArgument(name) {
  const value = optionalArgument(name);
  if (!value) {
    throw new Error(`${name} requires a value`);
  }
  return value;
}

function optionalArgument(name) {
  const index = process.argv.indexOf(name);
  const value = index >= 0 ? process.argv[index + 1] : undefined;
  return value && !value.startsWith("--") ? value : undefined;
}
