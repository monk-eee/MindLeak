// The ADR record, read from the ref that holds it.
//
// ADRs were read from the working tree, so the design record depended on which
// checkout was open. Under ADR-0038 concurrent work lives in many worktrees on
// different branches: measured across 84 of them on 2026-07-30, origin/main
// held 75 ADRs, the union across all 196 remote branches was also 75 -- so main
// is the complete record and nothing is ever branch-only -- yet 65 worktrees
// were missing between 1 and 26, and the checkout this extension reads held 49
// of 75 with no error of any kind.
//
// Deliberately free of `vscode` so it can be tested directly. The caller owns
// the fallback, because only the caller can report it to a human.
import { execFile } from "node:child_process";
import { promisify } from "node:util";

const run = promisify(execFile);

/** Where ADRs live, and the ref that holds the record. */
export const ADR_DIR = "docs/adr";
export const ADR_REF = "origin/main";

const ADR_PATH = /^docs\/adr\/\d{4}-.*\.md$/;

export interface AdrBlob {
  /** Repository-relative, forward-slashed: the id the design ledger stores. */
  path: string;
  text: string;
}

// A child git must never inherit the host's repository pointers, or it resolves
// a different repository than the one asked about.
const GIT_REPOSITORY_VARIABLES = [
  "GIT_DIR",
  "GIT_WORK_TREE",
  "GIT_COMMON_DIR",
  "GIT_INDEX_FILE",
  "GIT_OBJECT_DIRECTORY",
  "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

function gitEnvironment(): NodeJS.ProcessEnv {
  const isolated = { ...process.env };
  for (const variable of GIT_REPOSITORY_VARIABLES) delete isolated[variable];
  return isolated;
}

/**
 * Every ADR on `ref`, or `null` when the ref cannot be resolved.
 *
 * `null` rather than `[]` because the caller has to tell "there is no record
 * here" — a fresh clone with no remote — from "the record is empty". Reporting
 * the second as the first is how a partial record passes for a complete one.
 *
 * Contents come through a single `git cat-file --batch`. One `git show` per
 * file is the obvious spelling and costs a process spawn each: measured at
 * 10.7s for 75 ADRs against 0.36s to read them from disk. Batched it is 0.33s.
 */
export async function readAdrsOnRef(cwd: string, ref: string = ADR_REF): Promise<AdrBlob[] | null> {
  let listing: string;
  try {
    const { stdout } = await run("git", ["ls-tree", "-r", ref, "--", ADR_DIR], {
      cwd,
      env: gitEnvironment(),
      maxBuffer: 1 << 26,
    });
    listing = stdout;
  } catch {
    return null;
  }

  // `<mode> blob <sha>\t<path>`
  const entries = listing
    .split(/\r?\n/)
    .map((line) => /^\S+\s+blob\s+(\S+)\t(.+)$/.exec(line.trim()))
    .filter((match): match is RegExpExecArray => match !== null)
    .map((match) => ({ sha: match[1], path: match[2] }))
    .filter((entry) => ADR_PATH.test(entry.path))
    .sort((left, right) => left.path.localeCompare(right.path));

  if (entries.length === 0) {
    return [];
  }

  const blobs = await readBlobs(
    cwd,
    entries.map((entry) => entry.sha)
  );
  return entries.map((entry) => {
    const text = blobs.get(entry.sha);
    if (text === undefined) {
      throw new Error(`${entry.path}: unreadable on ${ref}`);
    }
    return { path: entry.path, text };
  });
}

/**
 * The contents of many blobs in one git process.
 *
 * The framing is `<sha> <type> <size>\n<size bytes>\n` and is parsed as bytes,
 * because the size git reports is in bytes: splitting the stream as text makes
 * one multi-byte character shift every record after it, which corrupts later
 * ADRs silently rather than failing on the one that caused it.
 */
async function readBlobs(cwd: string, shas: string[]): Promise<Map<string, string>> {
  const child = execFile("git", ["cat-file", "--batch"], {
    cwd,
    env: gitEnvironment(),
    maxBuffer: 1 << 28,
    encoding: "buffer",
  });

  const chunks: Buffer[] = [];
  child.stdout?.on("data", (chunk: Buffer) => chunks.push(chunk));
  const finished = new Promise<void>((resolve, reject) => {
    child.on("error", reject);
    child.on("close", () => resolve());
  });
  child.stdin?.end(`${shas.join("\n")}\n`);
  await finished;

  const out = Buffer.concat(chunks);
  const blobs = new Map<string, string>();
  let at = 0;
  while (at < out.length) {
    const newline = out.indexOf(0x0a, at);
    if (newline === -1) break;
    const [sha, , size] = out.subarray(at, newline).toString("utf8").split(" ");
    const start = newline + 1;
    const end = start + Number(size);
    blobs.set(sha, out.subarray(start, end).toString("utf8"));
    at = end + 1; // the newline git writes after each object
  }
  return blobs;
}
