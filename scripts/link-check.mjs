#!/usr/bin/env node
// Broken-link guard for the repository's markdown.
//
// A doc that links to a file which has since moved, been renamed, or deleted is
// a dead end a reader only finds by clicking. This most often bites ADR
// cross-references — an ADR is renamed and every inbound `[ADR-00NN](00NN-old-
// slug.md)` rots silently — and doc-to-source links when a module is split. The
// links are invisible in review because nothing follows them; this does.
//
// What counts as broken, and why it means what it appears to mean:
//   - Only *relative* links are checked. External (`http(s):`, `mailto:`,
//     `tel:`) and pure `#anchor` links are another tool's job.
//   - A target is resolved both relative to the linking file *and* to the repo
//     root, and is OK if either exists. The repo mixes both conventions (a
//     root-level file's `crates/…` link is already root-relative), and flagging
//     a convention difference as rot would bury the real thing under noise. A
//     target that resolves under neither is genuinely missing.
//   - A directory target is valid if the directory exists — `[gaps](gaps.d/)`
//     is a real link, not a broken one.
//   - Image links under `media/screenshots/` are skipped: those are captured
//     before a Marketplace publish (see the capture checklist beside them) and
//     the docs deliberately reference them ahead of time, showing alt text
//     until they land. Failing on them would fail on a documented plan.
//
// Cross-platform, dependency-free Node (toolchain rule). Reads the working tree
// via `git ls-files`, so it validates the state you are about to commit.
//   node scripts/link-check.mjs
//
// Scope: the *living* documentation. `docs/adr/` is deliberately excluded — an
// ADR is a historical record identified by its number, its cross-references
// capture other ADRs' titles as they were at the time, and some point at
// decisions that were renamed or never got their own file. Repairing those is a
// maintainer's call about intent, not a mechanical one, and is tracked
// separately; guarding them here would either fail on that backlog or force
// this tool to edit historical records to stay green. It still catches the rot
// that actually misleads a reader today: a living doc linking to a moved,
// renamed, or deleted file.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCREENSHOT_EXEMPT = /(^|\/)media\/screenshots\//;

/** Historical/append-only trees whose links are not the living-docs contract. */
const EXCLUDED_SOURCES = [/^docs\/adr\//];

/** Every tracked path, and the set of directories they imply. */
export function treeSets(files) {
  const fileSet = new Set(files);
  const dirSet = new Set();
  for (const f of files) {
    let d = path.posix.dirname(f);
    while (d && d !== "." && !dirSet.has(d)) {
      dirSet.add(d);
      d = path.posix.dirname(d);
    }
  }
  return { fileSet, dirSet };
}

const isSkippable = (target) =>
  /^(https?:|mailto:|tel:|#)/i.test(target) ||
  target === "..." ||
  /[<>|*\s]/.test(target) ||
  target.includes("${");

const LINK_RE = /\[[^\]]*\]\(\s*([^)]+?)\s*\)/g;

/** The broken relative links in one file's text, resolved against the tree. */
export function brokenLinksIn(rel, text, { fileSet, dirSet }) {
  const exists = (p) => {
    const n = p.replace(/\/+$/, "");
    return fileSet.has(n) || dirSet.has(n);
  };
  const dir = path.posix.dirname(rel);
  const out = [];
  text.split(/\r?\n/).forEach((line, i) => {
    LINK_RE.lastIndex = 0;
    let m;
    while ((m = LINK_RE.exec(line))) {
      let target = m[1].trim();
      const titled = target.search(/\s+["']/);
      if (titled !== -1) target = target.slice(0, titled).trim();
      if (isSkippable(target)) continue;
      target = target.split("#")[0];
      if (!target) continue;
      const fileRel = path.posix.normalize(path.posix.join(dir, target));
      const rootRel = path.posix.normalize(target.replace(/^\//, ""));
      if (SCREENSHOT_EXEMPT.test(fileRel) || SCREENSHOT_EXEMPT.test(rootRel))
        continue;
      if (exists(fileRel) || exists(rootRel)) continue;
      out.push({ file: rel, line: i + 1, target: m[1].trim() });
    }
  });
  return out;
}

export function checkRepo(root) {
  const tracked = execFileSync("git", ["ls-files"], {
    cwd: root,
    encoding: "utf8",
  })
    .trim()
    .split(/\r?\n/)
    .filter(Boolean);
  const sets = treeSets(tracked);
  const broken = [];
  for (const rel of tracked.filter((f) => f.endsWith(".md"))) {
    if (EXCLUDED_SOURCES.some((re) => re.test(rel))) continue;
    const text = readFileSync(path.join(root, rel), "utf8");
    broken.push(...brokenLinksIn(rel, text, sets));
  }
  return broken;
}

const thisFile = fileURLToPath(import.meta.url);
const invoked = process.argv[1] && path.resolve(process.argv[1]) === thisFile;
if (invoked || process.env.LINK_CHECK_RUN === "1") {
  const root = execFileSync("git", ["rev-parse", "--show-toplevel"], {
    encoding: "utf8",
  }).trim();
  const broken = checkRepo(root);
  if (broken.length === 0) {
    console.log("link-check: no broken relative links.");
    process.exit(0);
  }
  console.error(`link-check: ${broken.length} broken relative link(s):`);
  for (const b of broken)
    console.error(`  ${b.file}:${b.line}  ->  ${b.target}`);
  process.exit(1);
}
