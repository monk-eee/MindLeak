// Known gaps as per-gap fragments (the ADR-0056 treatment, applied to gaps).
//
// The Known gaps section of DEVELOPERS.md was one shared append-only list, so
// every branch that recorded a gap edited the same lines and every merge
// collided there — hand-resolved four times in a single session, each time
// producing a conflict that expressed no disagreement at all: two agents adding
// two unrelated observations to the same paragraph.
//
// ADR-0056 already solved this shape for CHANGELOG.md. A fragment is a new file
// per item, and two branches never write the same path.
//
// ONE DELIBERATE DIFFERENCE FROM changelog.d. A changelog fragment is temporary:
// `--release` folds it into CHANGELOG.md and deletes it. A gap has no release
// event — it is open until it is fixed — so folding would put the shared list
// straight back and the conflict with it. The fragments are therefore the
// source of truth, permanently, and DEVELOPERS.md points at them rather than
// holding a generated copy. Closing a gap deletes its fragment, which is
// attributable in the commit that fixes it.
//
// Platform-agnostic: node only. Usage:
//   node scripts/gaps.mjs --check     validate fragments (hook/CI)
//   node scripts/gaps.mjs --list      print every open gap, for reading

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

export const FRAGMENT_DIR = "gaps.d";

/** A fragment name is the gap's slug: lowercase, kebab, `.md`. */
export const isFragmentName = (name) =>
  /^[a-z0-9]+(-[a-z0-9]+)*\.md$/.test(name);

/**
 * Read every fragment, newest-agnostic and sorted by name so the rendered order
 * is stable. Unreadable or malformed fragments are collected rather than thrown,
 * so `--check` can report all of them at once instead of one per run.
 */
export const readFragments = (dir = FRAGMENT_DIR) => {
  if (!existsSync(dir)) return { gaps: [], files: [], problems: [] };
  const files = readdirSync(dir)
    .filter((name) => name.endsWith(".md") && name !== "README.md")
    .sort();

  const gaps = [];
  const problems = [];
  for (const name of files) {
    if (!isFragmentName(name)) {
      problems.push(
        `${name}: name must be <slug>.md, lowercase and kebab-case`,
      );
      continue;
    }
    const body = readFileSync(join(dir, name), "utf8").replace(/\s+$/, "");
    if (!/^- \*\*/m.test(body)) {
      problems.push(
        `${name}: must open with a "- **" bullet naming the gap and its status`,
      );
      continue;
    }
    gaps.push({ name, body });
  }
  return { gaps, files, problems };
};

/** Every open gap, as one markdown list. */
export const render = (gaps) => gaps.map((gap) => gap.body).join("\n\n");

const main = () => {
  const args = process.argv.slice(2);
  const { gaps, files, problems } = readFragments();

  if (problems.length) {
    console.error(`gaps: ${problems.length} unusable fragment(s)`);
    for (const problem of problems) console.error(`  ${problem}`);
    process.exit(1);
  }

  // An empty Known Gaps section is almost always a lie — DEVELOPERS.md says so
  // itself. A validator that passes over an empty directory would report success
  // for a repository that had quietly lost every gap it ever recorded, which is
  // the one result it must never give.
  if (gaps.length === 0) {
    console.error(
      `gaps: no fragments in ${FRAGMENT_DIR}/ — an empty Known Gaps section is almost\n` +
        `  always a lie, so this is treated as a missing directory rather than a clean bill\n` +
        `  of health. Record one, or say plainly in DEVELOPERS.md why there are none.`,
    );
    process.exit(1);
  }

  if (args.includes("--list")) {
    console.log(render(gaps));
    return;
  }

  if (args.includes("--check")) {
    console.log(`gaps: ${files.length} fragment(s) valid`);
    return;
  }

  console.log(
    [
      "gaps -- known gaps are fragments, so recording one never conflicts",
      "",
      "  node scripts/gaps.mjs --check    validate fragments (hook/CI)",
      "  node scripts/gaps.mjs --list     print every open gap",
      "",
      `Add a gap: write ${FRAGMENT_DIR}/<slug>.md opening with a "- **" bullet.`,
      "Close a gap: delete its fragment in the commit that fixes it.",
    ].join("\n"),
  );
};

if (
  process.argv[1] &&
  import.meta.url.endsWith(process.argv[1].replace(/\\/g, "/"))
) {
  main();
}
