// ADR link check. Refuses a `docs/adr/*.md` cross-reference whose target file
// does not exist.
//
// ADR filenames carry a slug, and a slug is prose: it gets reworded while the
// number stays put. A link written from memory of the *idea* rather than the
// filename — `0045-armed-means-finished.md` for a decision filed as
// `0045-a-fleet-is-a-distributed-system.md` — renders as an ordinary link and
// only fails when a reader clicks it. Four such links sat on main, and they were
// found by writing this check, not by reading.
//
// The number in the link text is not the target. `[ADR-0045](...)` says which
// decision is meant, and it is usually right while the path beside it is wrong,
// so this reports the mismatch rather than guessing a correction: a link may
// legitimately point at a decision whose slug this tool cannot infer.
//
// Platform-agnostic: git + node only. Usage:
//   node scripts/adr-link-check.mjs [<file> ...]
// With no arguments it checks every ADR, which is what CI and the hook want;
// with arguments it checks only those files, which is what pre-commit passes.

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const ADR_DIRECTORY = "docs/adr";
// Link targets are relative to docs/adr, so `](0045-slug.md)`.
const ADR_LINK = /\]\((\d{4}-[a-z0-9-]+\.md)(#[^)]*)?\)/g;

const adrFiles = () => {
  if (!existsSync(ADR_DIRECTORY)) return [];
  return readdirSync(ADR_DIRECTORY)
    .filter((name) => /^\d{4}-.+\.md$/.test(name))
    .map((name) => join(ADR_DIRECTORY, name));
};

const targets = process.argv.slice(2).length
  ? process.argv
      .slice(2)
      .map((path) => path.replace(/\\/g, "/"))
      .filter((path) => /^docs\/adr\/\d{4}-.+\.md$/.test(path))
      .filter((path) => existsSync(path))
  : adrFiles();

let broken = 0;
for (const file of targets) {
  const source = readFileSync(file, "utf8");
  for (const [, target] of source.matchAll(ADR_LINK)) {
    if (existsSync(join(ADR_DIRECTORY, target))) continue;
    broken += 1;
    console.error(
      `adr-link-check: ${file.replace(/\\/g, "/")} links to ${target}, which does not exist.`,
    );
  }
}

if (broken > 0) {
  console.error(
    `\n  ${broken} ADR link(s) point at a filename that is not there.` +
      "\n  The number in the link text is probably right and the path beside it" +
      "\n  stale: check docs/adr for the decision's real filename.\n",
  );
  process.exit(1);
}
