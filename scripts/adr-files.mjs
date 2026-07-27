// Read the ADR files on disk: number, title, and declared status.
//
// Two tools need this and they must agree. scripts/adr-index.mjs derives the
// index table from it; scripts/design-audit.mjs compares it against the design
// ledger. A second parser would drift from the first the moment either learned
// a new status, and the mismatch would look like real drift.
//
// Platform-agnostic: node only, no dependencies.

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

export const ADR_DIR = "docs/adr";

/** Statuses an ADR heading may declare. `Superseded by <ref>` is also valid. */
export const VALID_STATUS = new Set([
  "Proposed",
  "Accepted",
  "Rejected",
  "Deprecated",
]);

export const isSuperseded = (status) => /^Superseded/i.test(status);

const parse = (dir, file) => {
  const path = join(dir, file);
  const text = readFileSync(path, "utf8");
  const number = /^(\d{4})-/.exec(file)?.[1];

  // "# ADR-0039: Every waiver ends" -> "Every waiver ends"
  const heading = /^#\s+(.+?)\s*$/m.exec(text)?.[1];
  const title = heading?.replace(/^ADR-\d{4}\s*[:\u2014-]\s*/, "").trim();

  // "- Status: Accepted", or a wrapped
  //   "- Status: Superseded by
  //      [ADR-0038](0038-....md)"
  //
  // The continuation matters. ADR-0032 wraps its reference onto the next line,
  // and a status regex that stopped at the newline read it as a bare
  // "Superseded by" — which was then reported as a decision nobody could
  // attribute, when the answer was one line further down. Continuation lines
  // are indented and are not the next `- ` bullet or a heading.
  const status =
    /^[ \t]*-?[ \t]*(?:\*\*)?Status:(?:\*\*)?[ \t]*([^\r\n]*(?:\r?\n[ \t]+(?![-*#][ \t])[^\r\n]+)*)/im
      .exec(text)?.[1]
      ?.replace(/\*/g, "")
      .replace(/\s+/g, " ")
      .trim();

  if (!number || !title || !status) {
    throw new Error(
      `${file}: cannot read ${!number ? "number" : !title ? "title" : "status"}`,
    );
  }
  const head = status.split(/\s+/)[0];
  if (!VALID_STATUS.has(head) && !isSuperseded(status)) {
    throw new Error(`${file}: unrecognised status "${status}"`);
  }
  // `path` is the repository-relative id the design ledger stores as adr_path,
  // so it is always forward-slashed regardless of the host platform.
  return { number, file, path: path.split("\\").join("/"), title, status };
};

/** Every ADR in `dir`, ordered by number. Throws on an unreadable ADR. */
export const readAdrFiles = (dir = ADR_DIR) =>
  readdirSync(dir)
    .filter((f) => /^\d{4}-.*\.md$/.test(f))
    .sort()
    .map((f) => parse(dir, f));
