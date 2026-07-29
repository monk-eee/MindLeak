// Shell-plumbing guard (constitution: "Committed instructions carry no
// shell-specific plumbing"). The project already requires platform-agnostic
// operation, but that rule is stated as an outcome, so it is only noticed once
// something has broken on someone else's machine. This checks the plumbing
// itself, which is where it actually breaks.
//
// Two narrow rules, chosen so the guard fires on real problems and stays quiet
// otherwise -- a noisy guard is one people learn to bypass:
//
//   1. Documentation must not instruct in a single-platform shell. A fenced
//      block tagged powershell/pwsh/ps1/cmd/bat is a command the reader on
//      another OS cannot run. `bash` is the project's documented convention and
//      is deliberately allowed.
//   2. No inline interpreter one-liners. `node -e "..."` and friends embed a
//      program inside shell quoting, and every shell quotes differently: the
//      same line that works in one mangles its input in another, silently, and
//      the corruption only surfaces when someone reads the file it wrote. A
//      checked-in script has none of that ambiguity.
//
// Platform-agnostic: node only. Usage:
//   node scripts/no-shell-plumbing.mjs [<file> ...]
// With no arguments it checks every tracked text file.

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

// A fence tagged with a shell only one platform has.
const SINGLE_PLATFORM_FENCE =
  /^\s*```+\s*(powershell|pwsh|ps1|cmd|bat|batch)\b/i;

// An interpreter asked to run a program supplied as a quoted argument.
const INLINE_INTERPRETER = [
  /\bnode\s+(?:-\S+\s+)*(?:-e|--eval)\b/,
  /\b(?:python|python3)\s+(?:-\S+\s+)*-c\b/,
  /\b(?:ruby|perl)\s+(?:-\S+\s+)*-e\b/,
  /\b(?:powershell|pwsh)(?:\.exe)?\s+(?:-\S+\s+)*-(?:c|Command)\b/i,
  /\bcmd(?:\.exe)?\s+\/c\b/i,
];

// This guard and its tests necessarily contain the very patterns they look for.
const SELF = [
  "scripts/no-shell-plumbing.mjs",
  "scripts/no-shell-plumbing.test.mjs",
];

const TEXT = /\.(md|mjs|cjs|js|ts|yml|yaml|toml|rs|sql|sh)$|(^|\/)Makefile$/;

export const isSelfReferential = (path) =>
  SELF.some((s) => path.replace(/\\/g, "/").endsWith(s));

/// Every violation in one file. Pure, so the interesting cases are testable
/// without a repository.
export function findings(path, text) {
  if (isSelfReferential(path)) return [];
  const documentation = /\.md$/i.test(path);
  const out = [];
  let insideFence = false;

  text.split(/\r?\n/).forEach((line, index) => {
    const at = { path, line: index + 1, text: line.trim() };

    if (documentation && /^\s*```/.test(line)) {
      if (!insideFence && SINGLE_PLATFORM_FENCE.test(line)) {
        out.push({
          ...at,
          rule: "single-platform-fence",
          detail:
            "documentation instructs in a shell only one platform has; use a ```bash fence, or point at a checked-in script",
        });
      }
      insideFence = !insideFence;
      return;
    }

    // In documentation the interpreter rule applies only to actual instructions
    // -- text inside a fence. Prose has to be free to name the antipattern, or
    // the rule would forbid documenting itself, and this changelog entry would
    // be its own first violation.
    if (documentation && !insideFence) return;

    if (INLINE_INTERPRETER.some((pattern) => pattern.test(line))) {
      out.push({
        ...at,
        rule: "inline-interpreter",
        detail:
          "a program passed as a quoted shell argument; put it in a checked-in script and run that instead",
      });
    }
  });

  return out;
}

const trackedTextFiles = () =>
  execFileSync("git", ["ls-files"], { encoding: "utf8" })
    .split("\n")
    .map((f) => f.trim())
    .filter((f) => f && TEXT.test(f));

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  const requested = process.argv.slice(2).filter(Boolean);
  const files = (requested.length ? requested : trackedTextFiles()).filter(
    (f) => TEXT.test(f),
  );

  const violations = [];
  for (const file of files) {
    let text;
    try {
      text = readFileSync(file, "utf8");
    } catch {
      continue; // deleted or unreadable in this commit; nothing to check
    }
    violations.push(...findings(file, text));
  }

  if (violations.length > 0) {
    console.error(
      "no-shell-plumbing: refusing to commit shell-specific plumbing.\n",
    );
    for (const v of violations) {
      console.error(`  ${v.path}:${v.line}  [${v.rule}]`);
      console.error(`    ${v.text}`);
      console.error(`    ${v.detail}\n`);
    }
    console.error(
      "Committed scripts, docs, Makefile targets, and CI must work identically on\n" +
        "Linux, macOS, and Windows. Write the logic into a checked-in script and call it.",
    );
    process.exit(5);
  }
}
