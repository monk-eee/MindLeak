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

import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const ADR_DIR = "docs/adr";
const README = join(ADR_DIR, "README.md");
const VALID_STATUS = new Set([
  "Proposed",
  "Accepted",
  "Rejected",
  "Deprecated",
]);

const parse = (file) => {
  const text = readFileSync(join(ADR_DIR, file), "utf8");
  const number = /^(\d{4})-/.exec(file)?.[1];

  // "# ADR-0039: Every waiver ends" -> "Every waiver ends"
  const heading = /^#\s+(.+?)\s*$/m.exec(text)?.[1];
  const title = heading?.replace(/^ADR-\d{4}\s*[:\u2014-]\s*/, "").trim();

  // "- Status: Accepted" or "- Status: Superseded by [0038](...)"
  const status = /^\s*-?\s*(?:\*\*)?Status:(?:\*\*)?\s*([^\r\n]+)/im
    .exec(text)?.[1]
    ?.replace(/\*/g, "")
    .trim();

  if (!number || !title || !status) {
    throw new Error(
      `${file}: cannot read ${!number ? "number" : !title ? "title" : "status"}`,
    );
  }
  const head = status.split(/\s+/)[0];
  if (!VALID_STATUS.has(head) && !/^Superseded/i.test(status)) {
    throw new Error(`${file}: unrecognised status "${status}"`);
  }
  return { number, file, title, status };
};

const rows = readdirSync(ADR_DIR)
  .filter((f) => /^\d{4}-.*\.md$/.test(f))
  .sort()
  .map(parse);

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
