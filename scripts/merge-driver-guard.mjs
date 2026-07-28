// Merge-driver guard. Refuses any `merge=` declaration in a `.gitattributes`.
//
// A merge driver only exists in a local checkout. GitHub's "Update branch"
// button, the merge queue, and the merge itself all run server-side, where no
// driver is configured -- so an attribute that promises "keep both sides"
// silently does not apply, and the branch reports a conflict in a file the
// driver was supposed to make conflict-free.
//
// That is worse than having no driver at all. Without one you get an ordinary
// conflict you expect and resolve. With one you get a conflict that contradicts
// the repository's own configuration, in a file everybody edits, and the first
// instinct is to distrust the merge rather than the attribute. `merge=union` on
// CHANGELOG.md cost this repository an evening of phantom conflicts before the
// cause was found, and removing it is only half the fix: nothing stopped the
// next person from reading the same "keep both" wish and adding it back.
//
// The durable answer is per-change fragment files, which never collide because
// no two changes write the same path.
//
// Platform-agnostic: git + node only. Usage:
//   node scripts/merge-driver-guard.mjs [<file> ...]
// With no arguments it checks every tracked `.gitattributes`, which is what
// makes it useful outside the hook.

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";

/**
 * Every `merge=` declaration in one `.gitattributes`, as `{ line, number,
 * driver }`.
 *
 * Comments are ignored so the file can still explain why the rule exists --
 * a guard that forbids naming the thing it forbids makes the file unable to
 * document its own history.
 */
export function mergeDriverDeclarations(text) {
  const found = [];
  const lines = text.split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const code = line.split("#")[0];
    const match = /(?:^|\s)merge=(\S+)/.exec(code);
    if (match) {
      found.push({ line: line.trim(), number: index + 1, driver: match[1] });
    }
  }
  return found;
}

/** Every tracked `.gitattributes` path in the repository. */
function trackedAttributeFiles() {
  try {
    return execFileSync(
      "git",
      ["ls-files", "*.gitattributes", ".gitattributes"],
      {
        encoding: "utf8",
      },
    )
      .split(/\r?\n/)
      .filter(Boolean);
  } catch {
    return [".gitattributes"];
  }
}

function main() {
  const args = process.argv.slice(2);
  const files = (args.length ? args : trackedAttributeFiles()).filter((path) =>
    existsSync(path),
  );

  const violations = [];
  for (const file of files) {
    for (const found of mergeDriverDeclarations(readFileSync(file, "utf8"))) {
      violations.push({ file, ...found });
    }
  }

  if (!violations.length) {
    return;
  }

  console.error(
    `merge-driver-guard: ${violations.length} merge driver declaration(s); these do not apply on GitHub and cause conflicts they promise to prevent:`,
  );
  for (const violation of violations) {
    console.error(`  ${violation.file}:${violation.number}: ${violation.line}`);
  }
  console.error(
    "merge-driver-guard: use per-change fragment files instead; two changes never write the same path.",
  );
  process.exit(1);
}

// Only run the CLI when invoked directly, so the pure function stays importable.
if (
  import.meta.url === `file://${process.argv[1]}` ||
  process.argv[1]?.endsWith("merge-driver-guard.mjs")
) {
  main();
}
