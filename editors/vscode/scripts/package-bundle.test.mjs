import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { expect, test, vi } from "vitest";
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
        let readError;
        let enumerated = false;
        zip.on("error", (failure) => {
          readError = failure;
          zip.close();
        });
        zip.on("entry", (entry) => {
          entries.push({ name: entry.fileName, mode: entry.externalFileAttributes >>> 16 });
          zip.readEntry();
        });
        zip.on("end", () => {
          enumerated = true;
        });
        zip.on("close", () => {
          if (readError) reject(readError);
          else if (!enumerated) reject(new Error("archive closed before enumeration completed"));
          else resolve(entries);
        });
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

test.each(["success", "read error"])(
  "archive fixture cleanup waits for the file handle after %s",
  async (outcome) => {
    const open = yauzl.open;
    const remove = fs.rmSync;
    let closed = false;
    let cleanupAfterClose;
    let directory;
    let closure = Promise.resolve();
    vi.spyOn(yauzl, "open").mockImplementation((filename, options, ready) => {
      open(filename, options, (error, archive) => {
        if (archive) {
          closure = new Promise((resolve) =>
            archive.once("close", () => {
              closed = true;
              resolve();
            })
          );
          if (outcome === "read error") {
            archive.readEntry = () => {
              archive.emit("error", new Error("injected archive read failure"));
              archive.close();
            };
          }
        }
        ready(error, archive);
      });
    });
    vi.spyOn(fs, "rmSync").mockImplementation((target, options) => {
      directory = target;
      cleanupAfterClose = closed;
      return remove(target, options);
    });
    try {
      // Enumeration ended before the ZIP handle closed, so Windows cleanup
      // could fail with ENOTEMPTY despite successful archive assertions.
      if (outcome === "read error") {
        await expect(packageEntries(["README.txt"])).rejects.toThrow(
          "injected archive read failure"
        );
      } else {
        expect(await packageEntries(["README.txt"])).toHaveLength(1);
      }
      await closure;
      expect(cleanupAfterClose).toBe(true);
    } finally {
      vi.restoreAllMocks();
      await closure;
      if (directory) remove(directory, { recursive: true, force: true });
    }
  }
);
