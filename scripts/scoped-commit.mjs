// Commit-scope guard (ADR-0018). Stages and commits ONLY the paths you declare,
// via `git add -- <paths>` + `git commit -- <paths>` (pathspec) — never
// `git add -A` / `commit -a`. In a shared index this guarantees another agent's
// staged work is never swept into your commit or mis-attributed to your message.
// Any pre-existing staged path outside your declared set is reported and left
// uncommitted.
//
// Platform-agnostic: git + node only. Usage:
//   node scripts/scoped-commit.mjs -m "<message>" -- <path> [<path> ...]
//   node scripts/scoped-commit.mjs -F <msgfile>  -- <path> [<path> ...]

import { execFileSync } from "node:child_process";

const argv = process.argv.slice(2);
const bail = (message) => {
  console.error(`scoped-commit: ${message}`);
  process.exit(2);
};

const sep = argv.indexOf("--");
if (sep === -1) bail("missing `--` before the path list");
const opts = argv.slice(0, sep);
const paths = argv.slice(sep + 1).filter(Boolean);
if (paths.length === 0) bail("declare at least one path to commit");

let messageArgs = [];
const mi = opts.indexOf("-m");
const fi = opts.indexOf("-F");
if (mi !== -1 && opts[mi + 1]) messageArgs = ["-m", opts[mi + 1]];
else if (fi !== -1 && opts[fi + 1]) messageArgs = ["-F", opts[fi + 1]];
else bail("provide a message with -m <message> or -F <file>");

const run = (a, o = {}) => execFileSync("git", a, { stdio: "inherit", ...o });
const capture = (a) => execFileSync("git", a, { encoding: "utf8" }).trim();

// Stage only the declared paths.
run(["add", "--", ...paths]);

// Report any pre-existing staged paths outside the declared set — the pathspec
// commit below leaves them untouched (they are NOT committed).
const declared = paths.map((p) => p.replace(/\\/g, "/").replace(/\/+$/, ""));
const isDeclared = (file) =>
  declared.some((d) => file === d || file.startsWith(`${d}/`));
const staged = capture(["diff", "--cached", "--name-only"])
  .split("\n")
  .map((s) => s.trim())
  .filter(Boolean);
const foreign = staged.filter((file) => !isDeclared(file));
if (foreign.length) {
  console.warn(
    "scoped-commit: note — these staged paths are not yours and will be left uncommitted:",
  );
  for (const file of foreign) console.warn(`  ${file}`);
}

try {
  run(["commit", ...messageArgs, "--", ...paths]);
} catch (err) {
  process.exit(typeof err.status === "number" ? err.status : 1);
}
