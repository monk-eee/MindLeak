// Read the ADR record: number, title, and declared status.
//
// Three tools need this and they must agree. scripts/adr-index.mjs derives the
// index table from it; scripts/design-audit.mjs compares it against the design
// ledger; the Design Board shows it. A second parser would drift from the first
// the moment either learned a new status, and the mismatch would look like real
// drift.
//
// The record is read from `origin/main`, not from the working tree. Under
// ADR-0038 concurrent work lives in many worktrees on different branches, so
// `readdirSync` answers a different question in each one. Measured across 84
// attached worktrees on 2026-07-30: 75 ADRs on origin/main, and the union
// across all 196 remote branches also 75 -- main is the complete record and
// nothing is ever branch-only. Yet 65 of those worktrees were missing between 1
// and 26 ADRs, and the checkout the extension reads saw 49 of 75. A third of
// the design record absent, reported as if complete.
//
// Platform-agnostic: git + node only, no dependencies.

import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

export const ADR_DIR = "docs/adr";

/** The ref that holds the record. Acceptance happens to work already on main. */
export const ADR_REF = "origin/main";

// A child git process must never inherit the parent's repository pointers, or a
// hook-invoked run resolves the wrong repository entirely.
const GIT_REPOSITORY_VARIABLES = [
  "GIT_DIR",
  "GIT_WORK_TREE",
  "GIT_COMMON_DIR",
  "GIT_INDEX_FILE",
  "GIT_OBJECT_DIRECTORY",
  "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

/** git, run against `cwd` only, deaf to inherited repository pointers. */
export const isolatedGit = (gitArgs, cwd = process.cwd()) => {
  try {
    const isolated = { ...process.env };
    for (const variable of GIT_REPOSITORY_VARIABLES) delete isolated[variable];
    return execFileSync("git", gitArgs, {
      cwd,
      encoding: "utf8",
      stdio: "pipe",
      maxBuffer: 1 << 26,
      env: isolated,
    }).trim();
  } catch {
    return null;
  }
};

/** Statuses an ADR heading may declare. `Superseded by <ref>` is also valid. */
export const VALID_STATUS = new Set([
  "Proposed",
  "Accepted",
  "Rejected",
  "Deprecated",
]);

export const isSuperseded = (status) => /^Superseded/i.test(status);

const parse = (dir, file) =>
  parseAdrText(join(dir, file), file, readFileSync(join(dir, file), "utf8"));

/** Parse one ADR. `path` is only an identity; the text is supplied. */
const parseAdrText = (path, file, text) => {
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

/**
 * Every ADR on `ref`, ordered by number — the whole record, whatever this
 * worktree happens to have checked out.
 *
 * Falls back to the working tree when the ref cannot be resolved, which is the
 * honest answer in a fresh clone with no remote. It says so when it does:
 * falling back *silently* is the failure this exists to fix, because a partial
 * record that reports itself as complete makes every tool downstream state
 * confident nonsense about design drift.
 *
 * @returns {{files: object[], source: string, fellBack: boolean}}
 */
export const readAdrFilesFromMain = ({
  cwd = process.cwd(),
  ref = ADR_REF,
  warn = (message) => console.error(message),
} = {}) => {
  const listing = isolatedGit(
    ["ls-tree", "-r", "--name-only", ref, "--", ADR_DIR],
    cwd,
  );
  if (listing === null) {
    warn(
      `adr-files: cannot resolve ${ref}; reading the working tree instead. ` +
        `That is this checkout's subset of the record, not the record.`,
    );
    return {
      files: readAdrFiles(join(cwd, ADR_DIR)),
      source: "working tree",
      fellBack: true,
    };
  }

  const files = listing
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => /^docs\/adr\/\d{4}-.*\.md$/.test(line))
    .sort()
    .map((path) => {
      const text = isolatedGit(["show", `${ref}:${path}`], cwd);
      if (text === null) throw new Error(`${path}: unreadable on ${ref}`);
      return parseAdrText(path, path.slice(ADR_DIR.length + 1), text);
    });

  return { files, source: ref, fellBack: false };
};
