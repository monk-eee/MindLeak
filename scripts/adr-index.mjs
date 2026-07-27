// Generate the ADR index table in docs/adr/README.md from the ADR files
// themselves (ADR-0038 fleet hygiene).
//
// The index is derived data that was maintained by hand: number, title, and
// status all already live in each ADR. Every concurrent branch appended a row
// to the same table, so every merge conflicted on it — the same shared-counter
// shape as ADR numbers, and the same fix: stop hand-maintaining what can be
// computed.
//
// Platform-agnostic: node only. Usage:
//   node scripts/adr-index.mjs           rewrite the table
//   node scripts/adr-index.mjs --check   fail if it is out of date (hook mode)

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { ADR_DIR, readAdrFiles } from "./adr-files.mjs";

const README = join(ADR_DIR, "README.md");

const rows = readAdrFiles();

const table = [
  "| ADR | Title | Status |",
  "|---|---|---|",
  ...rows.map((r) => `| [${r.number}](${r.file}) | ${r.title} | ${r.status} |`),
].join("\n");

const original = readFileSync(README, "utf8");
const tablePattern =
  /\| ADR \| Title \| Status \|\r?\n\|---\|---\|---\|(?:\r?\n\|.*)*/;
if (!tablePattern.test(original)) {
  throw new Error("could not find the ADR table in docs/adr/README.md");
}
const updated = original.replace(tablePattern, table);

if (process.argv.includes("--check")) {
  if (updated !== original) {
    console.error(
      "adr-index: docs/adr/README.md is out of date.\n" +
        "  Run: node scripts/adr-index.mjs",
    );
    process.exit(1);
  }
  console.log(`adr-index: index matches ${rows.length} ADRs`);
  process.exit(0);
}

if (updated === original) {
  console.log(`adr-index: already up to date (${rows.length} ADRs)`);
} else {
  writeFileSync(README, updated);
  console.log(`adr-index: rewrote index from ${rows.length} ADRs`);
}
