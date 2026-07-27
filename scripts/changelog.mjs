// Assemble CHANGELOG.md from per-change fragments (ADR-0056).
//
// A shared append-only file is a serialisation point. `.gitattributes` declares
// `merge=union` for CHANGELOG.md, git honours it, and GitHub's merge machinery
// does not — so five pull requests in one day reported a conflict that did not
// exist, and auto-merge silently stopped working on each. A fragment is a new
// file per change, and two branches never write the same path.
//
// Platform-agnostic: node only. Usage:
//   node scripts/changelog.mjs --check              validate fragments (hook/CI)
//   node scripts/changelog.mjs --preview            print what would be released
//   node scripts/changelog.mjs --release 0.1.4      fold into a dated section

import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";

export const FRAGMENT_DIR = "changelog.d";
const CHANGELOG = "CHANGELOG.md";

/** Keep a Changelog sections, in the order they are rendered. */
export const SECTIONS = [
  "added",
  "changed",
  "deprecated",
  "removed",
  "fixed",
  "security",
];

const heading = (section) => section[0].toUpperCase() + section.slice(1);

/**
 * Parse one fragment filename into its section. The section is a prefix rather
 * than front matter or a directory so the name alone says what it is: a reviewer
 * reading the file list of a pull request sees `fixed-empty-parent.md` and knows
 * both the section and the subject without opening anything.
 */
export const sectionOf = (name) => {
  const match = /^([a-z]+)-[^/]+\.md$/.exec(name);
  const section = match?.[1];
  return section && SECTIONS.includes(section) ? section : null;
};

/** Every fragment, grouped by section, each entry trimmed of trailing blanks. */
export const readFragments = (dir = FRAGMENT_DIR) => {
  if (!existsSync(dir)) return { grouped: new Map(), files: [], problems: [] };
  const files = readdirSync(dir)
    .filter((name) => name.endsWith(".md") && name !== "README.md")
    .sort();

  const grouped = new Map();
  const problems = [];
  for (const name of files) {
    const section = sectionOf(name);
    if (!section) {
      problems.push(
        `${name}: name must be <section>-<slug>.md where section is one of ${SECTIONS.join(", ")}`,
      );
      continue;
    }
    const body = readFileSync(join(dir, name), "utf8").replace(/\s+$/, "");
    if (!/^- /m.test(body)) {
      problems.push(`${name}: must contain at least one "- " bullet`);
      continue;
    }
    if (!grouped.has(section)) grouped.set(section, []);
    grouped.get(section).push(body);
  }
  return { grouped, files, problems };
};

/** Render grouped fragments as changelog markdown. Empty sections are omitted. */
export const render = (grouped) =>
  SECTIONS.filter((section) => grouped.get(section)?.length)
    .map(
      (section) =>
        `### ${heading(section)}\n${grouped.get(section).join("\n")}`,
    )
    .join("\n\n");

/**
 * Merge already-written `## [Unreleased]` content with rendered fragments,
 * keeping one heading per section. Entries that landed before this convention
 * existed must not be lost at the first release that uses it.
 */
export const foldUnreleased = (unreleased, grouped) => {
  const existing = new Map();
  let current = null;
  for (const line of unreleased.split(/\r?\n/)) {
    const match = /^### (.+)$/.exec(line);
    if (match) {
      current = match[1].trim().toLowerCase();
      if (!existing.has(current)) existing.set(current, []);
      continue;
    }
    if (current && line.trim()) existing.get(current).push(line);
  }
  const merged = new Map();
  for (const section of new Set([...existing.keys(), ...grouped.keys()])) {
    const lines = [];
    if (existing.get(section)?.length)
      lines.push(existing.get(section).join("\n"));
    if (grouped.get(section)?.length)
      lines.push(grouped.get(section).join("\n"));
    merged.set(section, [lines.join("\n")]);
  }
  return merged;
};

const readChangelog = () => {
  const text = readFileSync(CHANGELOG, "utf8");
  const start = text.indexOf("## [Unreleased]");
  if (start < 0) throw new Error("CHANGELOG.md has no ## [Unreleased] section");
  const next = text.indexOf("\n## [", start + 1);
  return {
    head: text.slice(0, start),
    unreleased: text.slice(
      start + "## [Unreleased]".length,
      next < 0 ? undefined : next + 1,
    ),
    tail: next < 0 ? "" : text.slice(next + 1),
  };
};

const main = () => {
  const args = process.argv.slice(2);
  const { grouped, files, problems } = readFragments();

  if (problems.length) {
    console.error(`changelog: ${problems.length} unusable fragment(s)`);
    for (const problem of problems) console.error(`  ${problem}`);
    process.exit(1);
  }

  if (args.includes("--check")) {
    console.log(`changelog: ${files.length} fragment(s) valid`);
    return;
  }

  const releaseAt = args.indexOf("--release");
  if (releaseAt < 0 || !args[releaseAt + 1]) {
    console.log(render(grouped) || "changelog: no fragments");
    return;
  }

  const version = args[releaseAt + 1];
  const { head, unreleased, tail } = readChangelog();
  const body = render(foldUnreleased(unreleased, grouped));
  if (!body)
    throw new Error(
      "nothing to release: no fragments and no unreleased entries",
    );

  const date = new Date().toISOString().slice(0, 10);
  writeFileSync(
    CHANGELOG,
    `${head}## [Unreleased]\n\n## [${version}] - ${date}\n\n${body}\n\n${tail}`,
  );
  for (const name of files) rmSync(join(FRAGMENT_DIR, name));
  console.log(
    `changelog: released ${version} from ${files.length} fragment(s)`,
  );
};

if (process.argv[1]?.endsWith("changelog.mjs")) main();
