import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { expect, test } from "vitest";
import yauzl from "yauzl";

async function packageEntries(files, executables = []) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "package-bundle-"));
  try {
    for (const name of files) fs.writeFileSync(path.join(directory, name), `fixture:${name}`);
    const output = path.join(directory, "bundle.zip");
    execFileSync(
      process.execPath,
      [
        fileURLToPath(new URL("./package-bundle.mjs", import.meta.url)),
        "--source",
        directory,
        "--out",
        output,
        ...(executables.length ? ["--executables", ...executables] : []),
        "--files",
        ...files,
      ],
      { stdio: "pipe" }
    );
    return await new Promise((resolve, reject) => {
      yauzl.open(output, { lazyEntries: true }, (error, zip) => {
        if (error) return reject(error);
        const entries = [];
        zip.on("error", reject);
        zip.on("entry", (entry) => {
          entries.push({ name: entry.fileName, mode: entry.externalFileAttributes >>> 16 });
          zip.readEntry();
        });
        zip.on("end", () => resolve(entries));
        zip.readEntry();
      });
    });
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}

test("an Industrial archive preserves execute permission on every declared host command", async () => {
  const binaries = [
    "mindleak-mcp",
    "lodestar-mcp",
    "ackplane-mcp",
    "ackplane-supervisor",
    "register-me",
    "ackplane-workctl",
  ];
  const entries = await packageEntries(
    [...binaries, "install.mjs", "industrial-manifest.json"],
    binaries
  );
  expect(entries.map((entry) => entry.name)).toEqual([
    ...binaries,
    "install.mjs",
    "industrial-manifest.json",
  ]);
  for (const entry of entries) {
    expect(entry.mode).toBe(entry.name === "industrial-manifest.json" ? 0o100644 : 0o100755);
  }
});

test("the Local archive retains its original executable and document modes", async () => {
  const entries = await packageEntries([
    "mindleak-mcp",
    "lodestar-mcp",
    "install.mjs",
    "README.txt",
  ]);
  expect(entries.map((entry) => entry.mode)).toEqual([0o100755, 0o100755, 0o100755, 0o100644]);
});
